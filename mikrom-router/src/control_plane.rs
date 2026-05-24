#![allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::unused_async,
    clippy::unnecessary_semicolon,
    clippy::collapsible_if
)]

use crate::config::{DatabaseUrl, MasterKey, NatsUrl, RouterId};
use crate::health::RouterHealth;
use crate::runtime;
use crate::state::{Certificate, Route, State};
use crate::state_manager::{StateManager, StateVersions};
use crate::subjects as router_subjects;
use crate::wireguard::WireGuardManager;
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use mikrom_proto::scheduler::RouterHeartbeat;
use mikrom_proto::subjects as proto_subjects;
use pingora::lb::LoadBalancer;
use pingora::lb::health_check::TcpHealthCheck;
use pingora::lb::selection::RoundRobin;
use pingora::server::ShutdownWatch;
use pingora::services::background::BackgroundService;
use prost::Message;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info};

mod handlers;

async fn publish_best_effort(
    nats: &async_nats::Client,
    subject: impl Into<String>,
    payload: Vec<u8>,
    context: &'static str,
) {
    let subject = subject.into();
    if let Err(e) = nats.publish(subject.clone(), payload.into()).await {
        error!(%context, %subject, error = %e, "Failed to publish NATS message");
    }
}

async fn publish_response_best_effort<T: Message>(
    nats: &async_nats::Client,
    reply: async_nats::Subject,
    response: &T,
    context: &'static str,
) {
    let mut buf = Vec::new();
    if let Err(e) = response.encode(&mut buf) {
        error!(%context, reply = %reply, error = %e, "Failed to encode NATS reply");
        return;
    }

    publish_best_effort(nats, reply.to_string(), buf, context).await;
}

fn tls_alternative_cn_for_host(host: &str) -> Option<String> {
    match host {
        "registry.mikrom.spluca.org" => Some("registry.mikrom.es".to_string()),
        _ => None,
    }
}

struct LoadedState {
    state: State,
    versions: StateVersions,
}

pub struct ControlPlane {
    db_url: DatabaseUrl,
    nats_url: NatsUrl,
    nats_use_tls: bool,
    nats_certs_dir: Option<String>,
    master_key: MasterKey,
    state_manager: Arc<StateManager>,
    health: Arc<RouterHealth>,
    router_id: RouterId,
    advertise_address: String,
    data_dir: String,
    wg_manager: Arc<WireGuardManager>,
    wg_port: u16,
}

impl ControlPlane {
    #[must_use]
    pub fn new(
        db_url: DatabaseUrl,
        nats_url: NatsUrl,
        nats_use_tls: bool,
        nats_certs_dir: Option<String>,
        master_key: MasterKey,
        state_manager: Arc<StateManager>,
        health: Arc<RouterHealth>,
        router_id: RouterId,
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
            health,
            router_id,
            advertise_address,
            data_dir,
            wg_manager: Arc::new(WireGuardManager::new("wg-mikrom").with_listen_port(wg_port)),
            wg_port,
        }
    }

    async fn sync_full_state(&self, db: &PgPool) -> Result<()> {
        info!("Performing full state sync from database...");

        let snapshot = self.load_full_state(db).await?;

        info!(
            routes = snapshot.state.routes.len(),
            acme_tokens = snapshot.state.acme_tokens.len(),
            certificates = snapshot.state.certificates.len(),
            "Applying full state sync"
        );
        self.state_manager
            .replace_state(snapshot.state, snapshot.versions)
            .await?;
        info!("State sync complete.");

        Ok(())
    }

    async fn load_full_state(&self, db: &PgPool) -> Result<LoadedState> {
        let (routes, route_versions) = self.load_routes(db).await?;
        let (acme_tokens, acme_versions) = self.load_acme_tokens(db).await?;
        let (certificates, certificate_versions) = self.load_certificates(db).await?;

        Ok(LoadedState {
            state: State {
                routes,
                acme_tokens,
                certificates,
            },
            versions: StateVersions {
                route_versions,
                acme_versions,
                certificate_versions,
            },
        })
    }

    async fn load_routes(
        &self,
        db: &PgPool,
    ) -> Result<(HashMap<String, Route>, HashMap<String, i64>)> {
        let route_rows = sqlx::query(
            "SELECT hostname, target_url, EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at FROM routes",
        )
        .fetch_all(db)
        .await?;

        let mut route_targets: HashMap<String, (Vec<String>, bool)> = HashMap::new();
        let mut route_versions: HashMap<String, i64> = HashMap::new();
        for row in route_rows {
            use sqlx::Row;
            let host: String = row.get("hostname");
            let target: String = row.get("target_url");
            let updated_at: i64 = row.get("updated_at");

            let use_tls = target.starts_with("https://");
            let target = target
                .strip_prefix("https://")
                .or_else(|| target.strip_prefix("http://"))
                .unwrap_or(&target)
                .to_string();

            let entry = route_targets.entry(host.clone()).or_default();
            entry.0.push(target);
            entry.1 |= use_tls;
            route_versions
                .entry(host)
                .and_modify(|current| *current = (*current).max(updated_at))
                .or_insert(updated_at);
        }

        let mut routes: HashMap<String, Route> = HashMap::new();
        for (host, (targets, use_tls)) in route_targets {
            let mut lb = LoadBalancer::<RoundRobin>::try_from_iter(targets.as_slice())
                .context("Failed to create load balancer from targets")?;

            let mut hc = TcpHealthCheck::default();
            hc.consecutive_success = 1;
            hc.consecutive_failure = 2;

            lb.set_health_check(Box::new(hc));
            lb.health_check_frequency = Some(std::time::Duration::from_millis(250));
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

        Ok((routes, route_versions))
    }

    async fn load_acme_tokens(
        &self,
        db: &PgPool,
    ) -> Result<(HashMap<String, String>, HashMap<String, i64>)> {
        let acme_rows = sqlx::query(
            "SELECT token, key_auth, EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at FROM acme_challenges",
        )
            .fetch_all(db)
            .await?;

        let mut acme_tokens = HashMap::new();
        let mut acme_versions = HashMap::new();
        for row in acme_rows {
            use sqlx::Row;
            let token: String = row.get("token");
            let key_auth: String = row.get("key_auth");
            let updated_at: i64 = row.get("updated_at");
            acme_tokens.insert(token.clone(), key_auth);
            acme_versions.insert(token, updated_at);
        }

        Ok((acme_tokens, acme_versions))
    }

    async fn load_certificates(
        &self,
        db: &PgPool,
    ) -> Result<(HashMap<String, Certificate>, HashMap<String, i64>)> {
        let cert_rows = sqlx::query(
            "SELECT hostname, cert_chain, private_key, EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at FROM tls_certificates",
        )
        .fetch_all(db)
        .await?;

        let mut certificates = HashMap::new();
        let mut certificate_versions = HashMap::new();
        for row in cert_rows {
            use sqlx::Row;
            let domain: String = row.get("hostname");
            let cert_pem: String = row.get("cert_chain");
            let encrypted_key: String = row.get("private_key");
            let updated_at: i64 = row.get("updated_at");

            let key_pem = match crate::crypto::decrypt(&encrypted_key, self.master_key.as_str()) {
                Ok(key) => key,
                Err(e) => {
                    error!("Control Plane: Failed to decrypt private key for {domain}: {e}");
                    encrypted_key
                },
            };

            certificates.insert(
                domain.clone(),
                Certificate {
                    cert_pem,
                    key_pem,
                    parsed_chain: Vec::new(),
                    parsed_key: None,
                },
            );

            certificate_versions.insert(domain, updated_at);
        }

        Ok((certificates, certificate_versions))
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
        runtime::init_tracing_once("control-plane");

        // 0. Enable IPv6 forwarding (essential for WireGuard 6PN routing)
        if let Err(e) = self
            .set_proc_sysctl("net/ipv6/conf/all/forwarding", "1")
            .await
        {
            error!("Control Plane: Failed to enable IPv6 forwarding: {e}");
        }

        // Connect to database
        let db = runtime::connect_with_backoff(
            "Control Plane database",
            Duration::from_secs(5),
            || async {
                let pool = PgPool::connect(self.db_url.as_str())
                    .await
                    .with_context(|| {
                        format!("Failed to connect to database at {}", self.db_url.as_str())
                    })?;
                sqlx::migrate!("./migrations")
                    .run(&pool)
                    .await
                    .context("Database migration failed")?;
                Ok(pool)
            },
        )
        .await;
        info!("Control Plane: Connected to database and migrated.");

        let nats =
            runtime::connect_with_backoff("Control Plane NATS", Duration::from_secs(5), || async {
                crate::nats::connect_nats(
                    self.nats_url.as_str(),
                    self.nats_use_tls,
                    self.nats_certs_dir.as_deref(),
                )
                .await
            })
            .await;
        info!("Control Plane: Connected to NATS.");

        // 0. Initialize WireGuard
        let priv_key = match self.wg_manager.load_or_generate_key(&self.data_dir).await {
            Ok(k) => k,
            Err(e) => {
                error!("Control Plane: Failed to load/generate WireGuard key: {e}");
                self.health.set_startup_error(e.to_string());
                return;
            },
        };
        let pub_key = match self.wg_manager.get_public_key(&priv_key) {
            Ok(k) => k,
            Err(e) => {
                error!("Control Plane: Failed to get WireGuard public key: {e}");
                self.health.set_startup_error(e.to_string());
                return;
            },
        };
        if let Err(e) = self
            .wg_manager
            .init(&priv_key, self.router_id.as_str())
            .await
        {
            error!("Control Plane: Failed to initialize WireGuard: {e:?}");
            self.health.set_startup_error(e.to_string());
            return;
        }
        self.health.mark_wireguard_ready();
        let wg_ip = self
            .wg_manager
            .get_host_ipv6(self.router_id.as_str())
            .to_string();

        let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
        let mut heartbeat_interval = tokio::time::interval(std::time::Duration::from_secs(10));
        let mut pending = false;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);

        // 1. Initial sync
        if let Err(e) = self.sync_full_state(&db).await {
            error!("Control Plane: Initial state sync failed: {e}");
            self.health.set_startup_error(e.to_string());
        } else {
            self.health.clear_startup_error();
            self.health.mark_control_plane_synced();
        }

        // 2. Setup subscribers
        let mut router_sub = match nats
            .subscribe(router_subjects::control_plane_subject_wildcard().to_string())
            .await
        {
            Ok(s) => s,
            Err(e) => {
                error!("Control Plane: Failed to subscribe to router updates: {e}");
                return;
            },
        };

        let mut mesh_sub = match nats
            .subscribe(router_subjects::mesh_updates(self.router_id.as_str()).to_string())
            .await
        {
            Ok(s) => s,
            Err(e) => {
                error!("Control Plane: Failed to subscribe to mesh updates: {e}");
                return;
            },
        };

        self.health.mark_dependencies_ready();
        info!("Control Plane: Listening for updates...");

        loop {
            tokio::select! {
                // Heartbeat loop
                _ = heartbeat_interval.tick() => {
                    let heartbeat = RouterHeartbeat {
                        host_id: self.router_id.as_str().to_string(),
                        hostname: self.router_id.as_str().to_string(),
                        wireguard_pubkey: pub_key.clone(),
                        wireguard_ip: wg_ip.clone(),
                        wireguard_port: i32::from(self.wg_port),
                        advertise_address: self.advertise_address.clone(),
                    };
                    let mut buf = Vec::new();
                    if heartbeat.encode(&mut buf).is_ok() {
                        publish_best_effort(
                            &nats,
                            proto_subjects::SCHEDULER_ROUTER_HEARTBEAT,
                            buf,
                            "router-heartbeat",
                        )
                        .await;
                    }
                }
                // Router updates (routes, certs, acme)
                msg = router_sub.next() => {
                    if let Some(msg) = msg {
                        handlers::process_router_message(self, msg, &db, &nats, &tx).await;
                    }
                }
                // Mesh updates (peers)
                msg = mesh_sub.next() => {
                    if let Some(msg) = msg {
                        handlers::process_mesh_message(self, msg, &priv_key).await;
                    }
                }
                Some(()) = rx.recv() => {
                    pending = true;
                }
                _ = interval.tick() => {
                    if pending {
                        if let Err(e) = self.sync_full_state(&db).await {
                            error!("Control Plane: Failed to sync state after NATS update: {e}");
                        } else {
                            self.health.clear_startup_error();
                            self.health.mark_control_plane_synced();
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
