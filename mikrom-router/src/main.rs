pub mod control_plane;
pub mod proxy;
pub mod state;
pub mod state_manager;
pub mod telemetry;
pub mod tls;

#[cfg(test)]
mod proxy_tests;

#[cfg(test)]
mod integration_tests;

#[cfg(test)]
mod unit_tests;

use anyhow::Result;
use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::TracerProvider;
use pingora::listeners::tls::TlsSettings;
use pingora::prelude::*;
use pingora::server::configuration::ServerConf;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

fn init_tracing(service_name: &str) -> Result<()> {
    // 1. Create the OTLP SpanExporter with Tonic
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(
            std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:4317".to_string()),
        )
        .build()?;

    // 2. Create the TracerProvider and register the exporter
    let provider = TracerProvider::builder()
        .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
        .with_resource(Resource::new(vec![opentelemetry::KeyValue::new(
            "service.name",
            service_name.to_string(),
        )]))
        .build();

    // 3. Set the global tracer provider
    global::set_tracer_provider(provider.clone());

    // 4. Set global propagator for trace context propagation
    global::set_text_map_propagator(TraceContextPropagator::new());

    // 5. Setup tracing subscriber with OTel layer
    let tracer = provider.tracer("mikrom-router");
    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .with(telemetry)
        .init();

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let router_id = std::env::var("ROUTER_ID").unwrap_or_else(|_| {
        hostname::get().map_or_else(
            |_| "unknown-router".to_string(),
            |h| h.to_string_lossy().into_owned(),
        )
    });

    init_tracing(&format!("mikrom-router-{router_id}"))?;

    info!("Starting Mikrom Router (Pingora)...");

    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let nats_url = std::env::var("NATS_URL").expect("NATS_URL must be set");
    let cache_path = PathBuf::from(
        std::env::var("STATE_CACHE_PATH").unwrap_or_else(|_| "router-state.json".to_string()),
    );
    let acme_staging = std::env::var("ACME_STAGING").unwrap_or_default() == "true";
    let rps_limit = std::env::var("RPS_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    if acme_staging {
        info!(
            "ACME Staging mode is ENABLED. Certificates will be served from Let's Encrypt Staging (if available in DB)."
        );
    }

    // 1. Initialize State Manager
    let state_manager = Arc::new(state_manager::StateManager::new(cache_path)?);
    let state = state_manager.get_state();

    // 2. Start Control Plane in background
    let db = sqlx::PgPool::connect(&db_url).await?;

    info!("Running database migrations...");
    sqlx::migrate!("./migrations").run(&db).await?;

    let nats = async_nats::connect(&nats_url).await?;
    let cp_state_manager = state_manager.clone();
    let nats_for_cp = nats.clone();

    tokio::spawn(async move {
        let cp = control_plane::ControlPlane::new(db, nats_for_cp, cp_state_manager);
        if let Err(e) = cp.run().await {
            tracing::error!("Control plane error: {e}");
        }
    });

    // 3. Initialize Metrics
    let metrics_counters = Arc::new(proxy::RouterMetricsCounters::new());

    // 4. Start Telemetry Loop
    let metrics_for_telemetry = metrics_counters.clone();
    let router_id_for_telemetry = router_id.clone();
    tokio::spawn(async move {
        telemetry::start_telemetry_loop(nats, metrics_for_telemetry, router_id_for_telemetry).await;
    });

    // 5. Start Pingora Proxy
    let opt = Opt {
        upgrade: true,
        ..Default::default()
    };

    // Configure server settings for enterprise stability
    let conf = ServerConf {
        upgrade_sock: "/tmp/mikrom_router_upgrade.sock".to_string(),
        grace_period_seconds: Some(30), // Allow 30s for active requests to finish
        threads: std::env::var("ROUTER_THREADS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0), // 0 means use number of CPUs
        ..Default::default()
    };

    let mut server = Server::new_with_opt_and_conf(Some(opt), conf);
    server.bootstrap();

    let proxy_instance =
        proxy::MikromProxy::new(state.clone(), acme_staging, metrics_counters, rps_limit);

    let mut proxy_service = http_proxy_service(&server.configuration, proxy_instance);
    proxy_service.add_tcp("[::]:80");

    // HTTPS Listener with dynamic cert resolver
    let tls_handler = tls::MikromTlsHandler::new(state);
    let mut tls_settings = TlsSettings::with_callbacks(Box::new(tls_handler))?;
    tls_settings.enable_h2();

    proxy_service.add_tls_with_settings("[::]:443", None, tls_settings);

    server.add_service(proxy_service);
    server.run_forever();
}
