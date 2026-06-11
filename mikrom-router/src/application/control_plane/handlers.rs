use super::{ControlPlane, publish_response_best_effort};
use mikrom_proto::router::{
    AcmeChallengeUpdate, RouterConfigAck, RouterConfigUpdate, TlsCertificateUpdate,
};
use mikrom_proto::scheduler::NetworkMeshUpdate;
use mikrom_proto::subjects as proto_subjects;
use prost::Message;
use sqlx::PgPool;
use tracing::{debug, error, info};

fn schedule_state_resync(tx: &tokio::sync::mpsc::Sender<()>, context: &'static str, subject: &str) {
    if let Err(e) = tx.try_send(()) {
        debug!("Control Plane: Failed to schedule state resync after {context} for {subject}: {e}");
    }
}

pub(super) async fn process_router_message(
    control_plane: &ControlPlane,
    msg: async_nats::Message,
    db: &PgPool,
    nats: &async_nats::Client,
    tx: &tokio::sync::mpsc::Sender<()>,
) {
    match msg.subject.as_ref() {
        proto_subjects::ROUTER_CONFIG_UPDATED => {
            handle_router_config_update(control_plane, msg, db, nats, tx).await;
        },
        proto_subjects::ROUTER_TLS_CERT_UPDATED => {
            handle_tls_certificate_update(control_plane, msg, db, tx).await;
        },
        proto_subjects::ROUTER_ACME_CHALLENGE_UPDATED => {
            handle_acme_challenge_update(control_plane, msg, db, tx).await;
        },
        _ => {},
    }
}

pub(super) async fn process_mesh_message(
    control_plane: &ControlPlane,
    msg: async_nats::Message,
    priv_key: &str,
) {
    match NetworkMeshUpdate::decode(&msg.payload[..]) {
        Ok(update) => {
            debug!(
                "Control Plane: Received mesh update with {} peers",
                update.peers.len()
            );
            if let Err(e) = control_plane
                .wg_manager
                .update_peers(&update.peers, priv_key, control_plane.router_id.as_str())
                .await
            {
                error!("Control Plane: Failed to update WireGuard peers: {e}");
            }
        },
        Err(e) => error!("Control Plane: Failed to decode NetworkMeshUpdate: {e}"),
    }
}

async fn handle_router_config_update(
    control_plane: &ControlPlane,
    msg: async_nats::Message,
    db: &PgPool,
    nats: &async_nats::Client,
    tx: &tokio::sync::mpsc::Sender<()>,
) {
    let reply = msg.reply.clone();
    match RouterConfigUpdate::decode(&msg.payload[..]) {
        Ok(update) => {
            let response = apply_router_config_update(control_plane, update, db, tx).await;
            if let Some(reply) = reply {
                publish_response_best_effort(nats, reply, &response, "router-config-reply").await;
            }
        },
        Err(e) => error!("Control Plane: Failed to decode RouterConfigUpdate: {e}"),
    }
}

async fn apply_router_config_update(
    control_plane: &ControlPlane,
    update: RouterConfigUpdate,
    db: &PgPool,
    tx: &tokio::sync::mpsc::Sender<()>,
) -> RouterConfigAck {
    info!(
        "Control Plane: Received route update for {}",
        update.hostname
    );

    let previous_snapshot = control_plane.state_manager.snapshot().await;

    let applied = match control_plane
        .state_manager
        .update_route_targets(
            update.hostname.clone(),
            update.target_urls.clone(),
            update.timestamp,
        )
        .await
    {
        Ok(applied) => applied,
        Err(e) => {
            error!(
                "Control Plane: Fast-path route update failed for {}: {}",
                update.hostname, e
            );
            return RouterConfigAck {
                success: false,
                message: e.to_string(),
            };
        },
    };

    if !applied {
        debug!(
            "Control Plane: Ignoring stale route update for {} at timestamp {}",
            update.hostname, update.timestamp
        );
        return RouterConfigAck {
            success: true,
            message: String::new(),
        };
    }

    let db_result = async {
        let mut db_tx = db.begin().await?;
        sqlx::query("DELETE FROM routes WHERE hostname = $1")
            .bind(&update.hostname)
            .execute(&mut *db_tx)
            .await?;

        for target_url in &update.target_urls {
            sqlx::query(
                "INSERT INTO routes (hostname, target_url, updated_at) VALUES ($1, $2, TO_TIMESTAMP($3))",
            )
                .bind(&update.hostname)
                .bind(target_url)
                .bind(update.timestamp)
                .execute(&mut *db_tx)
                .await?;
        }

        db_tx.commit().await?;
        Ok::<(), sqlx::Error>(())
    }
    .await;

    match db_result {
        Ok(()) => {
            schedule_state_resync(tx, "updating routes", &update.hostname);
            RouterConfigAck {
                success: true,
                message: String::new(),
            }
        },
        Err(e) => {
            error!(
                "Control Plane: Failed to persist route update for {}: {}",
                update.hostname, e
            );

            if let Err(restore_err) = control_plane
                .state_manager
                .replace_state(previous_snapshot.0, previous_snapshot.1)
                .await
            {
                error!(
                    "Control Plane: Failed to restore in-memory route state for {} after DB error: {}",
                    update.hostname, restore_err
                );
            }

            RouterConfigAck {
                success: false,
                message: e.to_string(),
            }
        },
    }
}

async fn handle_tls_certificate_update(
    control_plane: &ControlPlane,
    msg: async_nats::Message,
    db: &PgPool,
    tx: &tokio::sync::mpsc::Sender<()>,
) {
    match TlsCertificateUpdate::decode(&msg.payload[..]) {
        Ok(update) => {
            info!(
                "Control Plane: Received TLS certificate update for {}",
                update.hostname
            );

            match crate::infrastructure::crypto::decrypt(
                &update.private_key,
                control_plane.master_key.as_str(),
            ) {
                Ok(key_pem) => {
                    match control_plane
                        .state_manager
                        .add_certificate(
                            update.hostname.clone(),
                            update.cert_chain.clone(),
                            key_pem,
                            update.timestamp,
                        )
                        .await
                    {
                        Ok(applied) => {
                            if !applied {
                                debug!(
                                    "Control Plane: Ignoring stale certificate update for {} at timestamp {}",
                                    update.hostname, update.timestamp
                                );
                                return;
                            }
                        },
                        Err(e) => {
                            error!(
                                "Control Plane: Fast-path certificate update failed for {}: {}",
                                update.hostname, e
                            );
                            return;
                        },
                    }
                },
                Err(e) => {
                    error!(
                        "Control Plane: Failed to decrypt received certificate for {}: {}",
                        update.hostname, e
                    );
                    return;
                },
            }

            match sqlx::query(
                "INSERT INTO tls_certificates (hostname, cert_chain, private_key, expires_at)
                 VALUES ($1, $2, $3, TO_TIMESTAMP($4))
                 ON CONFLICT (hostname) DO UPDATE SET cert_chain = EXCLUDED.cert_chain, private_key = EXCLUDED.private_key, expires_at = EXCLUDED.expires_at, updated_at = TO_TIMESTAMP($5)",
            )
            .bind(&update.hostname)
            .bind(&update.cert_chain)
            .bind(&update.private_key)
            .bind(update.expires_at)
            .bind(update.timestamp)
            .execute(db)
            .await
            {
                Ok(_) => {
                    schedule_state_resync(tx, "updating TLS certificate", &update.hostname);
                },
                Err(e) => error!(
                    "Control Plane: Failed to persist TLS certificate for {}: {}",
                    update.hostname, e
                ),
            }
        },
        Err(e) => error!("Control Plane: Failed to decode TlsCertificateUpdate: {e}"),
    }
}

async fn handle_acme_challenge_update(
    control_plane: &ControlPlane,
    msg: async_nats::Message,
    db: &PgPool,
    tx: &tokio::sync::mpsc::Sender<()>,
) {
    match AcmeChallengeUpdate::decode(&msg.payload[..]) {
        Ok(update) => {
            info!(
                "Control Plane: Received ACME challenge update: {}",
                update.token
            );

            if update.is_delete {
                match control_plane
                    .state_manager
                    .remove_acme_token(&update.token, update.timestamp)
                    .await
                {
                    Ok(applied) => {
                        if !applied {
                            debug!(
                                "Control Plane: Ignoring stale ACME delete for {} at timestamp {}",
                                update.token, update.timestamp
                            );
                            return;
                        }
                    },
                    Err(e) => {
                        error!(
                            "Control Plane: Failed to remove ACME token {} from memory: {}",
                            update.token, e
                        );
                        return;
                    },
                }
            } else {
                match control_plane
                    .state_manager
                    .add_acme_token(
                        update.token.clone(),
                        update.key_auth.clone(),
                        update.timestamp,
                    )
                    .await
                {
                    Ok(applied) => {
                        if !applied {
                            debug!(
                                "Control Plane: Ignoring stale ACME update for {} at timestamp {}",
                                update.token, update.timestamp
                            );
                            return;
                        }
                    },
                    Err(e) => {
                        error!(
                            "Control Plane: Failed to add ACME token {} to memory: {}",
                            update.token, e
                        );
                        return;
                    },
                }
            }

            let query = if update.is_delete {
                sqlx::query("DELETE FROM acme_challenges WHERE token = $1").bind(&update.token)
            } else {
                sqlx::query(
                        "INSERT INTO acme_challenges (token, key_auth, hostname, updated_at) VALUES ($1, $2, $3, TO_TIMESTAMP($4))
                         ON CONFLICT (token) DO UPDATE SET key_auth = EXCLUDED.key_auth, hostname = EXCLUDED.hostname, updated_at = EXCLUDED.updated_at",
                    )
                    .bind(&update.token)
                    .bind(&update.key_auth)
                    .bind(&update.hostname)
                    .bind(update.timestamp)
            };
            if let Err(e) = query.execute(db).await {
                error!(
                    "Control Plane: Failed to persist ACME challenge {}: {}",
                    update.token, e
                );
                schedule_state_resync(tx, "ACME persistence failure", &update.token);
            } else {
                info!(
                    "Control Plane: Persisted ACME challenge {} for host {}",
                    update.token, update.hostname
                );
                schedule_state_resync(tx, "updating ACME challenge", &update.token);
            }
        },
        Err(e) => error!("Control Plane: Failed to decode AcmeChallengeUpdate: {e}"),
    }
}
