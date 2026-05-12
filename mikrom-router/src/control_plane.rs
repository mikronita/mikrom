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
use mikrom_proto::router::{AcmeChallengeUpdate, RouterConfigUpdate, TlsCertificateUpdate};
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
use tracing::{error, info};

pub struct ControlPlane {
    db_url: String,
    nats_url: String,
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

        // Fetch routes
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
            // Create Pingora Load Balancer with Health Check
            let mut lb = LoadBalancer::<RoundRobin>::try_from_iter(targets.as_slice())
                .context("Failed to create load balancer from targets")?;

            let mut hc = TcpHealthCheck::default();
            hc.consecutive_success = 1;
            hc.consecutive_failure = 2;

            lb.set_health_check(Box::new(hc));
            lb.health_check_frequency = Some(std::time::Duration::from_secs(5));

            routes.insert(
                host.clone(),
                Route {
                    host,
                    targets,
                    lb: Arc::new(lb),
                    use_tls,
                },
            );
        }

        // Fetch ACME tokens
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

        // Fetch Certificates
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

            // Decrypt key
            let key_pem = match crate::crypto::decrypt(&encrypted_key, &self.master_key) {
                Ok(key) => key,
                Err(e) => {
                    error!("Control Plane: Failed to decrypt private key for {domain}: {e}");
                    // Fallback: assume it might be raw PEM for manual entries
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

        let new_state = State {
            routes,
            acme_tokens,
            certificates,
        };

        self.state_manager.update_state(new_state).await?;
        info!("State sync complete.");

        Ok(())
    }
}

#[async_trait]
impl BackgroundService for ControlPlane {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        crate::init_tracing_once("control-plane");

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
            match async_nats::connect(&self.nats_url).await {
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
                        match msg.subject.as_ref() {
                            subjects::ROUTER_CONFIG_UPDATED => {
                                if let Ok(update) = RouterConfigUpdate::decode(&msg.payload[..]) {
                                    info!("Control Plane: Received route update for {}", update.hostname);
                                    if let Some(target_url) = update.target_url {
                                        sqlx::query(
                                            "INSERT INTO routes (hostname, target_url) VALUES ($1, $2)
                                             ON CONFLICT (hostname) DO UPDATE SET target_url = EXCLUDED.target_url, updated_at = NOW()"
                                        )
                                        .bind(&update.hostname)
                                        .bind(&target_url)
                                        .execute(&db)
                                        .await
                                        .ok();
                                    } else {
                                        sqlx::query("DELETE FROM routes WHERE hostname = $1")
                                            .bind(&update.hostname)
                                            .execute(&db)
                                            .await
                                            .ok();
                                    }
                                    let _ = tx.try_send(());
                                }
                            }
                            subjects::ROUTER_TLS_CERT_UPDATED => {
                                if let Ok(update) = TlsCertificateUpdate::decode(&msg.payload[..]) {
                                    info!("Control Plane: Received TLS certificate update for {}", update.hostname);
                                    sqlx::query(
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
                                    .ok();
                                    if let Err(e) = self.sync_full_state(&db).await {
                                        error!("Control Plane: Failed to refresh state after TLS cert update: {e}");
                                    }
                                    let _ = tx.try_send(());
                                }
                            }
                            subjects::ROUTER_ACME_CHALLENGE_UPDATED => {
                                if let Ok(update) = AcmeChallengeUpdate::decode(&msg.payload[..]) {
                                    info!("Control Plane: Received ACME challenge update: {}", update.token);
                                    if update.is_delete {
                                        sqlx::query("DELETE FROM acme_challenges WHERE token = $1")
                                            .bind(&update.token)
                                            .execute(&db)
                                            .await
                                            .ok();
                                    } else {
                                        sqlx::query(
                                            "INSERT INTO acme_challenges (token, key_auth) VALUES ($1, $2)
                                             ON CONFLICT (token) DO UPDATE SET key_auth = EXCLUDED.key_auth"
                                        )
                                        .bind(&update.token)
                                        .bind(&update.key_auth)
                                        .execute(&db)
                                        .await
                                        .ok();
                                    }
                                    if let Err(e) = self.sync_full_state(&db).await {
                                        error!("Control Plane: Failed to refresh state after ACME challenge update: {e}");
                                    }
                                    let _ = tx.try_send(());
                                }
                            }
                            _ => {}
                        }
                    }
                }
                // Mesh updates (peers)
                msg = mesh_sub.next() => {
                    if let Some(msg) = msg {
                        if let Ok(update) = NetworkMeshUpdate::decode(&msg.payload[..]) {
                            info!("Control Plane: Received mesh update with {} peers", update.peers.len());
                            if let Err(e) = self.wg_manager.update_peers(&update.peers, &priv_key, &self.router_id).await {
                                error!("Control Plane: Failed to update WireGuard peers: {e}");
                            }
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
