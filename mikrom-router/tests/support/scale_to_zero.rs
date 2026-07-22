#![allow(dead_code, unreachable_pub)]

use async_trait::async_trait;
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::any,
};
use mikrom_proto::subjects;
use mikrom_router::application::proxy::{MikromProxy, RouterMetricsCounters};
use mikrom_router::application::traffic::RouterTrafficPublisher;
use mikrom_router::domain::health::RouterHealth;
use mikrom_router::domain::state::{Route, State};
use mikrom_scheduler::domain::AppRepository;
use mikrom_scheduler::domain::{AgentClient, AppConfig, DomainResult, VmConfig};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use pingora::lb::LoadBalancer;
use pingora::lb::selection::RoundRobin;
use pingora::prelude::*;
use prost::Message;
use std::collections::HashMap;
use std::fmt::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{RwLock, mpsc};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub struct TestEnv {
    pub proxy_url: String,
    pub upstream_addr: SocketAddr,
}

#[derive(Default)]
pub struct RecordingAgentClient {
    pub starts: AtomicUsize,
    pub resumes: AtomicUsize,
    pub pauses: AtomicUsize,
}

pub struct MemoryAppRepository {
    inner: RwLock<HashMap<String, AppConfig>>,
    pool: sqlx::PgPool,
}

impl MemoryAppRepository {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
            pool,
        }
    }

    pub async fn upsert(&self, config: AppConfig) {
        self.update_app_config(config).await.unwrap();
    }

    async fn mirror_to_api_db(&self, config: &AppConfig) -> anyhow::Result<()> {
        let app_id = uuid::Uuid::parse_str(&config.id)?;
        sqlx::query(
            r"
            UPDATE apps
            SET vpc_ipv6_prefix = $2,
                hostname = $3,
                desired_replicas = $4,
                min_replicas = $5,
                max_replicas = $6,
                autoscaling_enabled = $7,
                cpu_threshold = $8,
                mem_threshold = $9,
                last_router_traffic_at = $10,
                last_scaled_to_zero_at = $11,
                updated_at = NOW()
            WHERE id = $1
            ",
        )
        .bind(app_id)
        .bind(&config.vpc_ipv6_prefix)
        .bind(&config.hostname)
        .bind(config.desired_replicas.cast_signed())
        .bind(config.min_replicas.cast_signed())
        .bind(config.max_replicas.cast_signed())
        .bind(config.autoscaling_enabled)
        .bind(config.cpu_threshold)
        .bind(config.mem_threshold)
        .bind(config.last_router_traffic_at)
        .bind(config.last_scaled_to_zero_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

#[async_trait]
impl mikrom_scheduler::domain::AppRepository for MemoryAppRepository {
    async fn update_app_config(&self, config: AppConfig) -> anyhow::Result<()> {
        self.inner
            .write()
            .await
            .insert(config.id.to_string(), config.clone());
        self.mirror_to_api_db(&config).await?;
        Ok(())
    }

    async fn get_app_config(&self, app_id: &str) -> anyhow::Result<Option<AppConfig>> {
        Ok(self.inner.read().await.get(app_id).cloned())
    }

    async fn get_app_config_by_hostname(
        &self,
        hostname: &str,
    ) -> anyhow::Result<Option<AppConfig>> {
        Ok(self
            .inner
            .read()
            .await
            .values()
            .find(|app| app.hostname == hostname)
            .cloned())
    }

    async fn list_all_apps(&self) -> anyhow::Result<Vec<AppConfig>> {
        Ok(self.inner.read().await.values().cloned().collect())
    }

    async fn list_autoscaling_apps(&self) -> anyhow::Result<Vec<AppConfig>> {
        Ok(self
            .inner
            .read()
            .await
            .values()
            .filter(|app| app.autoscaling_enabled)
            .cloned()
            .collect())
    }

    async fn remove_app_config(&self, app_id: &str) -> anyhow::Result<()> {
        self.inner.write().await.remove(app_id);
        Ok(())
    }

    async fn remove_app_and_jobs_by_app(&self, app_id: &str) -> anyhow::Result<()> {
        self.inner.write().await.remove(app_id);
        Ok(())
    }
}

#[async_trait]
impl AgentClient for RecordingAgentClient {
    async fn start_vm(
        &self,
        _host_id: &str,
        _app_id: &str,
        _image: &str,
        _vm_id: &str,
        _config: &VmConfig,
    ) -> DomainResult<()> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn pause_vm(&self, _host_id: &str, _vm_id: &str) -> DomainResult<()> {
        self.pauses.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn resume_vm(&self, _host_id: &str, _vm_id: &str) -> DomainResult<()> {
        self.resumes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn stop_vm(&self, _host_id: &str, _vm_id: &str) -> DomainResult<()> {
        Ok(())
    }

    async fn delete_vm(
        &self,
        _host_id: &str,
        _vm_id: &str,
        _hv: mikrom_scheduler::domain::HypervisorType,
    ) -> DomainResult<()> {
        Ok(())
    }

    async fn check_health(&self, _host_id: &str, _vm_id: &str) -> DomainResult<bool> {
        Ok(true)
    }

    async fn update_firewall(
        &self,
        _host_id: &str,
        _vm_id: &str,
        _rules: Vec<mikrom_proto::scheduler::FirewallRule>,
    ) -> DomainResult<()> {
        Ok(())
    }

    async fn create_volume(
        &self,
        _host_id: &str,
        _volume_id: &str,
        _size_mib: u32,
        _pool_name: &str,
    ) -> DomainResult<()> {
        Ok(())
    }

    async fn create_snapshot(
        &self,
        _host_id: &str,
        _volume_id: &str,
        _snapshot_name: &str,
        _pool_name: &str,
    ) -> DomainResult<()> {
        Ok(())
    }

    async fn delete_volume(
        &self,
        _host_id: &str,
        _volume_id: &str,
        _pool_name: &str,
    ) -> DomainResult<()> {
        Ok(())
    }

    async fn delete_snapshot(
        &self,
        _host_id: &str,
        _volume_id: &str,
        _snapshot_name: &str,
        _pool_name: &str,
    ) -> DomainResult<()> {
        Ok(())
    }

    async fn restore_snapshot(
        &self,
        _host_id: &str,
        _volume_id: &str,
        _snapshot_name: &str,
        _pool_name: &str,
    ) -> DomainResult<()> {
        Ok(())
    }

    async fn clone_volume(
        &self,
        _host_id: &str,
        _source_volume_id: &str,
        _snapshot_name: &str,
        _target_volume_id: &str,
        _pool_name: &str,
    ) -> DomainResult<()> {
        Ok(())
    }

    async fn get_volume_usage(
        &self,
        _host_id: &str,
        _volume_id: &str,
        _pool_name: &str,
    ) -> DomainResult<(u64, u64)> {
        Ok((0, 0))
    }

    async fn vm_snapshot_create(&self, _h: &str, _v: &str, _s: &str) -> DomainResult<()> {
        Ok(())
    }

    async fn vm_snapshot_restore(&self, _h: &str, _v: &str, _s: &str) -> DomainResult<()> {
        Ok(())
    }

    async fn vm_snapshot_delete(&self, _h: &str, _v: &str, _s: &str) -> DomainResult<()> {
        Ok(())
    }

    async fn vm_snapshot_list(
        &self,
        _h: &str,
        _v: &str,
    ) -> DomainResult<Vec<mikrom_proto::agent::VmSnapshotInfo>> {
        Ok(vec![])
    }

    async fn attach_volume(
        &self,
        _h: &str,
        _v: &str,
        _vol: &str,
        _m: &str,
        _r: bool,
    ) -> DomainResult<()> {
        Ok(())
    }

    async fn detach_volume(&self, _h: &str, _v: &str, _vol: &str) -> DomainResult<()> {
        Ok(())
    }

    async fn start_migration(&self, _h: &str, _v: &str, _th: &str, _tu: &str) -> DomainResult<()> {
        Ok(())
    }

    async fn cancel_migration(&self, _h: &str, _v: &str) -> DomainResult<()> {
        Ok(())
    }

    async fn query_migration(&self, _h: &str, _v: &str) -> DomainResult<String> {
        Ok("completed".to_string())
    }

    async fn set_balloon(&self, _h: &str, _v: &str, _s: u32) -> DomainResult<()> {
        Ok(())
    }

    async fn query_balloon(&self, _h: &str, _v: &str) -> DomainResult<(u32, u32)> {
        Ok((512, 512))
    }
}

fn init_test_tracing() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        use opentelemetry::trace::TracerProvider as _;
        use opentelemetry_sdk::trace::SdkTracerProvider;

        opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

        let provider = SdkTracerProvider::builder().build();
        let tracer = provider.tracer("mikrom-router-test");
        let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);

        let _ = tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer())
            .with(telemetry)
            .try_init();
    });
}

async fn dummy_upstream_handler(headers: HeaderMap) -> (StatusCode, String) {
    let mut echo = String::new();
    for (name, value) in &headers {
        let _ = writeln!(echo, "{name}: {}", value.to_str().unwrap_or(""));
    }
    (StatusCode::OK, echo)
}

#[allow(clippy::too_many_lines)]
pub async fn setup_test_env(rps_limit: isize, use_ipv6: bool) -> Option<TestEnv> {
    init_test_tracing();

    let app = Router::new().fallback(any(dummy_upstream_handler));
    let bind_addr = if use_ipv6 { "[::1]:0" } else { "127.0.0.1:0" };
    let listener = match tokio::net::TcpListener::bind(bind_addr).await {
        Ok(listener) => listener,
        Err(err) => {
            tracing::warn!(
                bind_addr = %bind_addr,
                error = %err,
                "Skipping router integration test environment because the sandbox does not allow binding"
            );
            return None;
        },
    };
    let upstream_addr = match listener.local_addr() {
        Ok(addr) => addr,
        Err(err) => {
            tracing::warn!(
                bind_addr = %bind_addr,
                error = %err,
                "Skipping router integration test environment because the upstream socket could not be inspected"
            );
            return None;
        },
    };

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let state = Arc::new(RwLock::new(State::default()));
    let metrics = Arc::new(RouterMetricsCounters::new());
    let health = Arc::new(RouterHealth::new());
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let traffic_bridge_nats = async_nats::connect(nats_url).await.ok();
    let (traffic_tx, traffic_rx) = mpsc::channel(1024);
    tokio::spawn(async move {
        let mut rx: mpsc::Receiver<mikrom_proto::router::RouterTrafficEvent> = traffic_rx;
        while let Some(event) = rx.recv().await {
            if let Some(client) = &traffic_bridge_nats {
                let mut buf = Vec::new();
                if event.encode(&mut buf).is_ok() {
                    let _ = client
                        .publish(subjects::ROUTER_TRAFFIC_EVENT, buf.into())
                        .await;
                }
            }
        }
    });

    let (proxy_addr_str, proxy_port) = match std::net::TcpListener::bind(bind_addr) {
        Ok(listener) => {
            let addr = listener.local_addr().unwrap();
            (addr.to_string(), addr.port())
        },
        Err(err) => {
            tracing::warn!(
                bind_addr = %bind_addr,
                error = %err,
                "Skipping router integration test environment because the proxy listener could not be bound"
            );
            return None;
        },
    };
    let proxy_url = if use_ipv6 {
        format!("http://[::1]:{proxy_port}")
    } else {
        format!("http://127.0.0.1:{proxy_port}")
    };

    let targets = vec![upstream_addr.to_string()];
    let lb = LoadBalancer::<RoundRobin>::try_from_iter(targets.as_slice()).unwrap();
    let lb_arc = Arc::new(lb);
    {
        let mut s = state.write().await;
        let route = Route {
            host: "localhost".to_string(),
            targets: targets.clone(),
            lb: lb_arc,
            use_tls: false,
            tls_alternative_cn: None,
        };

        s.routes.insert("localhost".to_string(), route.clone());
        s.routes.insert("127.0.0.1".to_string(), route.clone());
        s.routes.insert("[::1]".to_string(), route.clone());
        s.routes
            .insert(format!("localhost:{proxy_port}"), route.clone());
        s.routes
            .insert(format!("127.0.0.1:{proxy_port}"), route.clone());
        s.routes.insert(format!("[::1]:{proxy_port}"), route);
        drop(s);
    }

    let traffic_publisher = Arc::new(RouterTrafficPublisher::new(
        "router-test".into(),
        traffic_tx,
    ));
    let proxy = MikromProxy::new(
        state.clone(),
        health,
        false,
        String::new(),
        String::new(),
        "127.0.0.1:5001,[::1]:5001".to_string(),
        "127.0.0.1:5173,[::1]:5173".to_string(),
        None,
        metrics,
        Some(traffic_publisher),
        rps_limit,
        mikrom_router::application::proxy::RouterTimeouts::default(),
    );

    std::thread::spawn(move || {
        let mut my_server = Server::new(None).expect("Failed to create server");
        my_server.bootstrap();

        let mut proxy_service = http_proxy_service(&my_server.configuration, proxy);
        proxy_service.add_tcp(&proxy_addr_str);

        my_server.add_service(proxy_service);
        my_server.run_forever();
    });

    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    Some(TestEnv {
        proxy_url,
        upstream_addr,
    })
}
