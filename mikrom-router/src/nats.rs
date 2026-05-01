use async_nats::Client;
use mikrom_proto::router::{AcmeChallengeUpdate, RouterConfigUpdate, TlsCertificateUpdate};
use moka::future::Cache;
use prost::Message;
use sqlx::PgPool;
use tokio_stream::StreamExt;
use tracing::{error, info};

pub fn start_nats_listener(nats_url: String, db: PgPool, cache: Cache<String, String>) {
    tokio::spawn(async move {
        loop {
            info!("Connecting to NATS for updates at {}...", nats_url);
            let nats_client = match async_nats::connect(&nats_url).await {
                Ok(client) => client,
                Err(e) => {
                    error!("Failed to connect to NATS, retrying in 5s: {}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                },
            };

            if let Err(e) = listen_for_updates(nats_client, &db, &cache).await {
                error!("NATS listener error: {}, reconnecting in 5s...", e);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    });
}

async fn listen_for_updates(
    nats_client: Client,
    db: &PgPool,
    cache: &Cache<String, String>,
) -> anyhow::Result<()> {
    let mut config_sub = nats_client
        .subscribe(mikrom_proto::subjects::ROUTER_CONFIG_UPDATED)
        .await
        .map_err(|_| anyhow::anyhow!("Failed to subscribe to config updates"))?;
    let mut tls_sub = nats_client
        .subscribe(mikrom_proto::subjects::ROUTER_TLS_CERT_UPDATED)
        .await
        .map_err(|_| anyhow::anyhow!("Failed to subscribe to TLS updates"))?;
    let mut acme_sub = nats_client
        .subscribe(mikrom_proto::subjects::ROUTER_ACME_CHALLENGE_UPDATED)
        .await
        .map_err(|_| anyhow::anyhow!("Failed to subscribe to ACME updates"))?;

    info!("Listening for router config, TLS, and ACME updates via NATS...");

    loop {
        tokio::select! {
            Some(msg) = config_sub.next() => {
                handle_config_update(&msg.payload, db, cache).await;
            },
            Some(msg) = tls_sub.next() => {
                handle_tls_update(&msg.payload, db, cache).await;
            },
            Some(msg) = acme_sub.next() => {
                handle_acme_update(&msg.payload, db).await;
            },
            else => break,
        }
    }
    Ok(())
}

async fn handle_config_update(payload: &[u8], db: &PgPool, cache: &Cache<String, String>) {
    if let Ok(update) = RouterConfigUpdate::decode(payload) {
        info!(
            "Received router update for {}: {:?}",
            update.hostname, update.target_url
        );
        let result = if let Some(target) = update.target_url {
            sqlx::query(
                "INSERT INTO routes (hostname, target_url, updated_at) \
                 VALUES ($1, $2, TO_TIMESTAMP($3)) \
                 ON CONFLICT (hostname) DO UPDATE SET \
                 target_url = EXCLUDED.target_url, updated_at = EXCLUDED.updated_at \
                 WHERE EXCLUDED.updated_at > routes.updated_at",
            )
            .bind(&update.hostname)
            .bind(&target)
            .bind(update.timestamp)
            .execute(db)
            .await
        } else {
            sqlx::query("DELETE FROM routes WHERE hostname = $1 AND updated_at <= TO_TIMESTAMP($2)")
                .bind(&update.hostname)
                .bind(update.timestamp)
                .execute(db)
                .await
        };

        if let Err(e) = result {
            error!("Failed to update local routes table: {}", e);
        } else {
            cache.invalidate(&update.hostname).await;
        }
    }
}

async fn handle_tls_update(payload: &[u8], db: &PgPool, cache: &Cache<String, String>) {
    if let Ok(update) = TlsCertificateUpdate::decode(payload) {
        info!("Received TLS certificate update for {}", update.hostname);
        let result = sqlx::query(
            "INSERT INTO tls_certificates (hostname, cert_chain, private_key, expires_at) \
             VALUES ($1, $2, $3, TO_TIMESTAMP($4)) \
             ON CONFLICT (hostname) DO UPDATE SET \
             cert_chain = EXCLUDED.cert_chain, private_key = EXCLUDED.private_key, \
             expires_at = EXCLUDED.expires_at, updated_at = NOW()",
        )
        .bind(&update.hostname)
        .bind(&update.cert_chain)
        .bind(&update.private_key)
        .bind(update.expires_at)
        .execute(db)
        .await;

        if let Err(e) = result {
            error!("Failed to update local tls_certificates table: {}", e);
        } else {
            // Invalidate route cache to ensure any cached resolution picks up new state if needed,
            // though TLS resolution happens separately.
            cache.invalidate(&update.hostname).await;
        }
    }
}

async fn handle_acme_update(payload: &[u8], db: &PgPool) {
    if let Ok(update) = AcmeChallengeUpdate::decode(payload) {
        info!("Received ACME challenge update for token: {}", update.token);
        let result = if update.is_delete {
            sqlx::query("DELETE FROM acme_challenges WHERE token = $1")
                .bind(&update.token)
                .execute(db)
                .await
        } else {
            sqlx::query(
                "INSERT INTO acme_challenges (token, key_auth, hostname) \
                 VALUES ($1, $2, $3) \
                 ON CONFLICT (token) DO UPDATE SET \
                 key_auth = EXCLUDED.key_auth, hostname = EXCLUDED.hostname",
            )
            .bind(&update.token)
            .bind(&update.key_auth)
            .bind(&update.hostname)
            .execute(db)
            .await
        };

        if let Err(e) = result {
            error!("Failed to update local acme_challenges table: {}", e);
        }
    }
}
