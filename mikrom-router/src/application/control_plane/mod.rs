#![allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::unused_async,
    clippy::unnecessary_semicolon,
    clippy::collapsible_if
)]

use crate::app::config::{DatabaseUrl, MasterKey, NatsUrl, RouterId};
use crate::app::runtime;
use crate::domain::health::RouterHealth;
use crate::domain::state::{Certificate, Route, State};
use crate::domain::subjects as router_subjects;
use crate::infrastructure::persistence::state_manager::StateManager;
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
    wg_manager: Arc<mikrom_network::WireGuardManager>,
    wg_port: u16,
    startup_connect_timeout: Duration,
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
        startup_connect_timeout: Duration,
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
            wg_manager: Arc::new(
                mikrom_network::WireGuardManager::new("wg-mikrom").with_listen_port(wg_port),
            ),
            wg_port,
            startup_connect_timeout,
        }
    }

    async fn sync_full_state(&self, db: &PgPool) -> Result<()> {
        info!("Control Plane: Syncing full state from database...");

        let mut state = State::default();

        // Load routes.
        let routes_rows = sqlx::query(
            "SELECT hostname, target_url, EXTRACT(EPOCH FROM updated_at)::BIGINT as ts FROM routes",
        )
        .fetch_all(db)
        .await?;

        let mut route_targets: HashMap<String, (Vec<String>, i64)> = HashMap::new();
        for row in routes_rows {
            use sqlx::Row;
            let hostname: String = row.get("hostname");
            let target_url: String = row.get("target_url");
            let ts: i64 = row.get("ts");
            let entry = route_targets.entry(hostname).or_insert((Vec::new(), ts));
            entry.0.push(target_url);
            entry.1 = entry.1.max(ts);
        }

        for (host, (targets, _ts)) in route_targets {
            let mut normalized_targets = Vec::new();
            let mut use_tls = false;

            for target in &targets {
                let (normalized, has_tls) = normalize_route_target(target);
                use_tls |= has_tls;
                normalized_targets.push(normalized);
            }

            let mut lb = LoadBalancer::<RoundRobin>::try_from_iter(normalized_targets.as_slice())?;
            let mut hc = TcpHealthCheck::default();
            hc.consecutive_success = 1;
            hc.consecutive_failure = 2;
            lb.set_health_check(Box::new(hc));
            lb.health_check_frequency = Some(Duration::from_millis(250));

            state.routes.insert(
                host.clone(),
                Route {
                    host: host.clone(),
                    targets: normalized_targets,
                    lb: Arc::new(lb),
                    use_tls,
                    tls_alternative_cn: tls_alternative_cn_for_host(&host),
                },
            );
        }

        // Load ACME tokens.
        let acme_rows = sqlx::query("SELECT token, key_auth FROM acme_challenges")
            .fetch_all(db)
            .await?;
        for row in acme_rows {
            use sqlx::Row;
            state
                .acme_tokens
                .insert(row.get("token"), row.get("key_auth"));
        }

        // Load certificates.
        let cert_rows =
            sqlx::query("SELECT hostname, cert_chain, private_key FROM tls_certificates")
                .fetch_all(db)
                .await?;
        for row in cert_rows {
            use sqlx::Row;
            let hostname: String = row.get("hostname");
            let cert_chain: String = row.get("cert_chain");
            let private_key: String = row.get("private_key");
            match crate::infrastructure::crypto::decrypt(&private_key, self.master_key.as_str()) {
                Ok(key_pem) => {
                    state.certificates.insert(
                        hostname,
                        Certificate {
                            cert_pem: cert_chain,
                            key_pem,
                            parsed_chain: Vec::new(),
                            parsed_key: None,
                        },
                    );
                },
                Err(e) => error!(
                    "Control Plane: Failed to decrypt certificate for {}: {}",
                    hostname, e
                ),
            }
        }

        self.state_manager.update_state(state).await?;
        Ok(())
    }
}

fn normalize_route_target(target: &str) -> (String, bool) {
    if let Some(rest) = target.strip_prefix("https://") {
        return (rest.to_string(), true);
    }

    if let Some(rest) = target.strip_prefix("http://") {
        return (rest.to_string(), false);
    }

    (target.to_string(), false)
}

#[async_trait]
impl BackgroundService for ControlPlane {
    async fn start(&self, mut _shutdown: ShutdownWatch) {
        info!("Control Plane: Starting...");

        let db = runtime::connect_with_backoff(
            "Control Plane DB",
            self.startup_connect_timeout,
            || async {
                let pool = tokio::time::timeout(
                    self.startup_connect_timeout,
                    PgPool::connect(self.db_url.as_str()),
                )
                .await
                .context("Database connection attempt timed out")?
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

        let nats = runtime::connect_with_backoff(
            "Control Plane NATS",
            self.startup_connect_timeout,
            || async {
                tokio::time::timeout(
                    self.startup_connect_timeout,
                    crate::infrastructure::nats::connect_nats(
                        self.nats_url.as_str(),
                        self.nats_use_tls,
                        self.nats_certs_dir.as_deref(),
                    ),
                )
                .await
                .context("NATS connection attempt timed out")?
            },
        )
        .await;
        info!("Control Plane: Connected to NATS.");

        let priv_key = match mikrom_network::KeyManager::load_or_generate_key(
            &self.data_dir,
            &mikrom_network::FileWireGuardKeyStore,
        )
        .await
        {
            Ok(k) => k,
            Err(e) => {
                error!("Control Plane: Failed to load/generate WireGuard key: {e}");
                self.health.set_startup_error(e.to_string());
                return;
            },
        };
        let pub_key = match mikrom_network::KeyManager::get_public_key(&priv_key) {
            Ok(k) => k,
            Err(e) => {
                error!("Control Plane: Failed to get WireGuard public key: {e}");
                self.health.set_startup_error(e.to_string());
                return;
            },
        };
        let wg_ip = self
            .wg_manager
            .get_host_ipv6(self.router_id.as_str())
            .to_string();

        let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
        let mut heartbeat_interval = tokio::time::interval(std::time::Duration::from_secs(10));
        let mut pending = false;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);

        // Perform the initial sync.
        if let Err(e) = self.sync_full_state(&db).await {
            error!("Control Plane: Initial state sync failed: {e}");
            self.health.set_startup_error(e.to_string());
        } else {
            self.health.clear_startup_error();
            self.health.mark_control_plane_synced();
        }

        // Set up subscribers.
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

        let wg_manager = self.wg_manager.clone();
        let wg_health = self.health.clone();
        let wg_router_id = self.router_id.clone();
        let wg_priv_key = priv_key.clone();
        info!("Control Plane: Starting WireGuard initialization in the background...");
        // Keep NATS subscriptions and ACME state processing live even if WireGuard init stalls.
        tokio::spawn(async move {
            if let Err(e) = wg_manager.init(&wg_priv_key, wg_router_id.as_str()).await {
                error!("Control Plane: Failed to initialize WireGuard: {e:?}");
                wg_health.set_startup_error(e.to_string());
                return;
            }

            wg_health.mark_wireguard_ready();
        });

        loop {
            tokio::select! {
                // Heartbeat loop.
                _ = heartbeat_interval.tick() => {
                    if !self.health.is_wireguard_ready() {
                        continue;
                    }

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

                // Internal trigger for state sync.
                _ = rx.recv() => {
                    pending = true;
                }

                // Batch state syncs.
                _ = interval.tick(), if pending => {
                    if let Err(e) = self.sync_full_state(&db).await {
                        error!("Control Plane: State sync failed: {e}");
                    }
                    pending = false;
                }

                // NATS router updates.
                Some(msg) = router_sub.next() => {
                    handlers::process_router_message(self, msg, &db, &nats, &tx).await;
                }

                // NATS mesh updates.
                Some(msg) = mesh_sub.next() => {
                    handlers::process_mesh_message(self, msg, &priv_key).await;
                }

                // Shutdown.
                _ = _shutdown.changed() => {
                    info!("Control Plane: Shutting down...");
                    break;
                }
            }
        }
    }
}

impl Clone for ControlPlane {
    fn clone(&self) -> Self {
        Self {
            db_url: self.db_url.clone(),
            nats_url: self.nats_url.clone(),
            nats_use_tls: self.nats_use_tls,
            nats_certs_dir: self.nats_certs_dir.clone(),
            master_key: self.master_key.clone(),
            state_manager: self.state_manager.clone(),
            health: self.health.clone(),
            router_id: self.router_id.clone(),
            advertise_address: self.advertise_address.clone(),
            data_dir: self.data_dir.clone(),
            wg_manager: self.wg_manager.clone(),
            wg_port: self.wg_port,
            startup_connect_timeout: self.startup_connect_timeout,
        }
    }
}
