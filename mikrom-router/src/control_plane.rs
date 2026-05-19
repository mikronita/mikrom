#![allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::unused_async,
    clippy::unnecessary_semicolon,
    clippy::collapsible_if
)]

use crate::state::{Certificate, Route, State};
use crate::state_manager::StateManager;
use crate::wireguard::WireGuardManager;
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use mikrom_proto::router::{
    AcmeChallengeUpdate, RouterConfigAck, RouterConfigUpdate, TlsCertificateUpdate,
};
use mikrom_proto::scheduler::{NetworkMeshUpdate, RouterHeartbeat};
use mikrom_proto::subjects;
use pingora::lb::LoadBalancer;
use pingora::lb::health_check::TcpHealthCheck;
use pingora::lb::selection::RoundRobin;
use pingora::server::ShutdownWatch;
use pingora::services::background::BackgroundService;
use prost::Message;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, info};

fn tls_alternative_cn_for_host(host: &str) -> Option<String> {
    match host {
        "registry.mikrom.spluca.org" => Some("registry.mikrom.es".to_string()),
        _ => None,
    }
}

pub struct ControlPlane {
    db_url: String,
    nats_url: String,
    nats_use_tls: bool,
    nats_certs_dir: Option<String>,
    master_key: String,
    state_manager: Arc<StateManager>,
    router_id: String,
    advertise_address: String,
    data_dir: String,
    wg_manager: Arc<WireGuardManager>,
    wg_port: u16,
}

impl ControlPlane {
    #[must_use]
    pub fn new(
        db_url: String,
        nats_url: String,
        nats_use_tls: bool,
        nats_certs_dir: Option<String>,
        master_key: String,
        state_manager: Arc<StateManager>,
        router_id: String,
        advertise_address: String,
        data_dir: String,
        wg_port: u16,
    ) -> Self {
        Self {
            db_url,
            nats_url,
            nats_use_tls,
            nats_certs_dir,
            master_key,
            state_manager,
            router_id,
            advertise_address,
            data_dir,
            wg_manager: Arc::new(WireGuardManager::new("wg-mikrom").with_listen_port(wg_port)),
            wg_port,
        }
    }

    pub async fn run(&self) -> Result<()> {
        // This method is now replaced by BackgroundService::start,
        // but kept for compatibility or tests if needed.
        Ok(())
    }

    async fn sync_full_state(&self, db: &PgPool) -> Result<()> {
        info!("Performing full state sync from database...");

        let routes = self.load_routes(db).await?;
        let acme_tokens = self.load_acme_tokens(db).await?;
        let certificates = self.load_certificates(db).await?;

        let new_state = State {
            routes,
            acme_tokens,
            certificates,
        };

        self.state_manager.update_state(new_state).await?;
        info!("State sync complete.");

        Ok(())
    }

    async fn load_routes(&self, db: &PgPool) -> Result<HashMap<String, Route>> {
        let route_rows = sqlx::query("SELECT hostname, target_url FROM routes")
            .fetch_all(db)
            .await?;

        let mut route_targets: HashMap<String, (Vec<String>, bool)> = HashMap::new();
        for row in route_rows {
            use sqlx::Row;
            let host: String = row.get("hostname");
            let target: String = row.get("target_url");

            let use_tls = target.starts_with("https://");
            let target = target
                .strip_prefix("https://")
                .or_else(|| target.strip_prefix("http://"))
                .unwrap_or(&target)
                .to_string();

            let entry = route_targets.entry(host).or_default();
            entry.0.push(target);
            entry.1 = use_tls;
        }

        let mut routes: HashMap<String, Route> = HashMap::new();
        for (host, (targets, use_tls)) in route_targets {
            let mut lb = LoadBalancer::<RoundRobin>::try_from_iter(targets.as_slice())
                .context("Failed to create load balancer from targets")?;

            let mut hc = TcpHealthCheck::default();
            hc.consecutive_success = 1;
            hc.consecutive_failure = 2;

            lb.set_health_check(Box::new(hc));
            lb.health_check_frequency = Some(std::time::Duration::from_secs(5));
            let tls_alternative_cn = tls_alternative_cn_for_host(&host);

            routes.insert(
                host.clone(),
                Route {
                    host,
                    targets,
                    lb: Arc::new(lb),
                    use_tls,
                    tls_alternative_cn,
                },
            );
        }

        Ok(routes)
    }

    async fn load_acme_tokens(&self, db: &PgPool) -> Result<HashMap<String, String>> {
        let acme_rows = sqlx::query("SELECT token, key_auth FROM acme_challenges")
            .fetch_all(db)
            .await?;

        let mut acme_tokens = HashMap::new();
        for row in acme_rows {
            use sqlx::Row;
            let token: String = row.get("token");
            let key_auth: String = row.get("key_auth");
            acme_tokens.insert(token, key_auth);
        }

        Ok(acme_tokens)
    }

    async fn load_certificates(&self, db: &PgPool) -> Result<HashMap<String, Certificate>> {
        let cert_rows =
            sqlx::query("SELECT hostname, cert_chain, private_key FROM tls_certificates")
                .fetch_all(db)
                .await?;

        let mut certificates = HashMap::new();
        for row in cert_rows {
            use sqlx::Row;
            let domain: String = row.get("hostname");
            let cert_pem: String = row.get("cert_chain");
            let encrypted_key: String = row.get("private_key");

            let key_pem = match crate::crypto::decrypt(&encrypted_key, &self.master_key) {
                Ok(key) => key,
                Err(e) => {
                    error!("Control Plane: Failed to decrypt private key for {domain}: {e}");
                    encrypted_key
                },
            };

            certificates.insert(
                domain,
                Certificate {
                    cert_pem,
                    key_pem,
                    parsed_chain: Vec::new(),
                    parsed_key: None,
                },
            );
        }

        Ok(certificates)
    }

    async fn set_proc_sysctl(&self, key: &str, value: &str) -> Result<()> {
        let path = format!("/proc/sys/{key}");
        tokio::fs::write(&path, value)
            .await
            .with_context(|| format!("Failed to write sysctl {key}={value} at {path}"))?;
        Ok(())
    }
}

#[async_trait]
impl BackgroundService for ControlPlane {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        crate::init_tracing_once("control-plane");

        // 0. Enable IPv6 forwarding (essential for WireGuard 6PN routing)
        if let Err(e) = self
            .set_proc_sysctl("net/ipv6/conf/all/forwarding", "1")
            .await
        {
            error!("Control Plane: Failed to enable IPv6 forwarding: {e}");
        }

        // Connect to database
        let db = loop {
            match PgPool::connect(&self.db_url).await {
                Ok(pool) => {
                    info!("Control Plane: Connected to database.");
                    // Run migrations
                    if let Err(e) = sqlx::migrate!("./migrations").run(&pool).await {
                        error!("Control Plane: Database migration failed: {e}");
                    } else {
                        info!("Control Plane: Database migrations completed.");
                        break pool;
                    }
                },
                Err(e) => {
                    error!("Control Plane: Failed to connect to database: {e}. Retrying in 5s...");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                },
            }
        };

        // Connect to NATS
        let nats = loop {
            match crate::nats::connect_nats(
                &self.nats_url,
                self.nats_use_tls,
                self.nats_certs_dir.as_deref(),
            )
            .await
            {
                Ok(client) => {
                    info!("Control Plane: Connected to NATS.");
                    break client;
                },
                Err(e) => {
                    error!("Control Plane: Failed to connect to NATS: {e}. Retrying in 5s...");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                },
            };
        };

        // 0. Initialize WireGuard
        let priv_key = match self.wg_manager.load_or_generate_key(&self.data_dir).await {
            Ok(k) => k,
            Err(e) => {
                error!("Control Plane: Failed to load/generate WireGuard key: {e}");
                return;
            },
        };
        let pub_key = match self.wg_manager.get_public_key(&priv_key) {
            Ok(k) => k,
            Err(e) => {
                error!("Control Plane: Failed to get WireGuard public key: {e}");
                return;
            },
        };
        if let Err(e) = self.wg_manager.init(&priv_key, &self.router_id).await {
            error!("Control Plane: Failed to initialize WireGuard: {e}");
        }
        let wg_ip = self.wg_manager.get_host_ipv6(&self.router_id);

        let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
        let mut heartbeat_interval = tokio::time::interval(std::time::Duration::from_secs(10));
        let mut pending = false;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);

        // 1. Initial sync
        if let Err(e) = self.sync_full_state(&db).await {
            error!("Control Plane: Initial state sync failed: {e}");
        }

        // 2. Setup subscribers
        let mut router_sub = match nats.subscribe("mikrom.router.>").await {
            Ok(s) => s,
            Err(e) => {
                error!("Control Plane: Failed to subscribe to router updates: {e}");
                return;
            },
        };

        let mesh_subject = format!("mikrom.scheduler.network.mesh.{}", self.router_id);
        let mut mesh_sub = match nats.subscribe(mesh_subject.clone()).await {
            Ok(s) => s,
            Err(e) => {
                error!("Control Plane: Failed to subscribe to mesh updates: {e}");
                return;
            },
        };

        info!("Control Plane: Listening for updates...");

        loop {
            tokio::select! {
                // Heartbeat loop
                _ = heartbeat_interval.tick() => {
                    let heartbeat = RouterHeartbeat {
                        host_id: self.router_id.clone(),
                        hostname: self.router_id.clone(),
                        wireguard_pubkey: pub_key.clone(),
                        wireguard_ip: wg_ip.clone(),
                        wireguard_port: i32::from(self.wg_port),
                        advertise_address: self.advertise_address.clone(),
                    };
                    let mut buf = Vec::new();
                    if heartbeat.encode(&mut buf).is_ok() {
                        let _ = nats.publish("mikrom.scheduler.router.heartbeat", buf.into()).await;
                    }
                }
                // Router updates (routes, certs, acme)
                msg = router_sub.next() => {
                    if let Some(msg) = msg {
                        let state_manager = self.state_manager.clone();
                        let db = db.clone();
                        let nats = nats.clone();
                        let tx = tx.clone();
                        let master_key = self.master_key.clone();

                        match msg.subject.as_ref() {
                            subjects::ROUTER_CONFIG_UPDATED => {
                                match RouterConfigUpdate::decode(&msg.payload[..]) {
                                    Ok(update) => {
                                        info!("Control Plane: Received route update for {}", update.hostname);

                                        // First, delete existing targets for this host to maintain eventual consistency with the desired state
                                        let _ = sqlx::query("DELETE FROM routes WHERE hostname = $1")
                                            .bind(&update.hostname)
                                            .execute(&db)
                                            .await;

                                        let response = if !update.target_urls.is_empty() {
                                            let mut success = true;
                                            let mut last_error = String::new();

                                            for target_url in &update.target_urls {
                                                if let Err(e) = sqlx::query(
                                                    "INSERT INTO routes (hostname, target_url) VALUES ($1, $2)",
                                                )
                                                .bind(&update.hostname)
                                                .bind(target_url)
                                                .execute(&db)
                                                .await {
                                                    error!("Control Plane: Failed to persist target {} for {}: {}", target_url, update.hostname, e);
                                                    success = false;
                                                    last_error = e.to_string();
                                                }
                                            }

                                            if success {
                                                let _ = tx.try_send(());
                                                RouterConfigAck {
                                                    success: true,
                                                    message: String::new(),
                                                }
                                            } else {
                                                RouterConfigAck {
                                                    success: false,
                                                    message: last_error,
                                                }
                                            }
                                        } else {
                                            // targets are empty, we already deleted them above
                                            let _ = tx.try_send(());
                                            RouterConfigAck {
                                                success: true,
                                                message: String::new(),
                                            }
                                        };

                                        if let Some(reply) = msg.reply {
                                            let mut buf = Vec::new();
                                            if response.encode(&mut buf).is_ok() {
                                                let _ = nats.publish(reply, buf.into()).await;
                                            }
                                        }
                                    },
                                    Err(e) => error!("Control Plane: Failed to decode RouterConfigUpdate: {}", e),
                                }
                            }
                            subjects::ROUTER_TLS_CERT_UPDATED => {
                                match TlsCertificateUpdate::decode(&msg.payload[..]) {
                                    Ok(update) => {
                                        info!("Control Plane: Received TLS certificate update for {}", update.hostname);

                                        // FAST-PATH: Decrypt and update state immediately
                                        match crate::crypto::decrypt(&update.private_key, &master_key) {
                                            Ok(key_pem) => {
                                                if let Err(e) = state_manager.add_certificate(
                                                    update.hostname.clone(),
                                                    update.cert_chain.clone(),
                                                    key_pem
                                                ).await {
                                                    error!("Control Plane: Fast-path certificate update failed for {}: {}", update.hostname, e);
                                                } else {
                                                    info!("Control Plane: Successfully applied fast-path certificate for {}", update.hostname);
                                                }
                                            },
                                            Err(e) => error!("Control Plane: Failed to decrypt received certificate for {}: {}", update.hostname, e),
                                        }

                                        match sqlx::query(
                                            "INSERT INTO tls_certificates (hostname, cert_chain, private_key, expires_at)
                                             VALUES ($1, $2, $3, TO_TIMESTAMP($4))
                                             ON CONFLICT (hostname) DO UPDATE SET cert_chain = EXCLUDED.cert_chain, private_key = EXCLUDED.private_key, expires_at = EXCLUDED.expires_at, updated_at = NOW()"
                                        )
                                        .bind(&update.hostname)
                                        .bind(&update.cert_chain)
                                        .bind(&update.private_key)
                                        .bind(update.expires_at)
                                        .execute(&db)
                                        .await
                                        {
                                            Ok(_) => {
                                                let _ = tx.try_send(());
                                            },
                                            Err(e) => error!(
                                                "Control Plane: Failed to persist TLS certificate for {}: {}",
                                                update.hostname, e
                                            ),
                                        }
                                    },
                                    Err(e) => error!("Control Plane: Failed to decode TlsCertificateUpdate: {}", e),
                                }
                            }
                            subjects::ROUTER_ACME_CHALLENGE_UPDATED => {
                                match AcmeChallengeUpdate::decode(&msg.payload[..]) {
                                    Ok(update) => {
                                        info!("Control Plane: Received ACME challenge update: {}", update.token);

                                        // FAST-PATH: Update in-memory state immediately
                                        if update.is_delete {
                                            let _ = state_manager.remove_acme_token(&update.token).await;
                                        } else {
                                            let _ = state_manager.add_acme_token(update.token.clone(), update.key_auth.clone()).await;
                                        }

                                        let query = if update.is_delete {
                                            sqlx::query("DELETE FROM acme_challenges WHERE token = $1")
                                                .bind(&update.token)
                                        } else {
                                            sqlx::query(
                                                "INSERT INTO acme_challenges (token, key_auth, hostname) VALUES ($1, $2, $3)
                                                 ON CONFLICT (token) DO UPDATE SET key_auth = EXCLUDED.key_auth, hostname = EXCLUDED.hostname"
                                            )
                                            .bind(&update.token)
                                            .bind(&update.key_auth)
                                            .bind(&update.hostname)
                                        };
                                        if let Err(e) = query.execute(&db).await {
                                            error!(
                                                "Control Plane: Failed to persist ACME challenge {}: {}",
                                                update.token, e
                                            );
                                        } else {
                                            let _ = tx.try_send(());
                                        }
                                    },
                                    Err(e) => error!("Control Plane: Failed to decode AcmeChallengeUpdate: {}", e),
                                }
                            }
                            _ => {}
                        }
                    }
                }
                // Mesh updates (peers)
                msg = mesh_sub.next() => {
                    if let Some(msg) = msg {
                        match NetworkMeshUpdate::decode(&msg.payload[..]) {
                            Ok(update) => {
                                debug!("Control Plane: Received mesh update with {} peers", update.peers.len());
                                if let Err(e) = self.wg_manager.update_peers(&update.peers, &priv_key, &self.router_id).await {
                                    error!("Control Plane: Failed to update WireGuard peers: {e}");
                                }
                            },
                            Err(e) => error!("Control Plane: Failed to decode NetworkMeshUpdate: {}", e),
                        }
                    }
                }
                Some(()) = rx.recv() => {
                    pending = true;
                }
                _ = interval.tick() => {
                    if pending {
                        if let Err(e) = self.sync_full_state(&db).await {
                            error!("Control Plane: Failed to sync state after NATS update: {e}");
                        }
                        pending = false;
                    }
                }
                _ = shutdown.changed() => {
                    info!("Control Plane: Shutting down...");
                    break;
                }
            }
        }
    }
}
