#![allow(clippy::map_unwrap_or, clippy::uninlined_format_args)]

pub mod control_plane;
pub mod crypto;
pub mod nats;
pub mod proxy;
pub mod state;
pub mod state_manager;
pub mod telemetry;
pub mod tls;
pub mod upstream_ca;
pub mod wireguard;

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

pub static TRACING_INIT: std::sync::Once = std::sync::Once::new();

pub fn init_tracing_once(router_id: &str) {
    TRACING_INIT.call_once(|| {
        if let Err(e) = init_tracing(&format!("mikrom-router-{router_id}")) {
            eprintln!("Failed to initialize tracing: {e}");
        }
    });
}

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

fn main() -> Result<()> {
    let router_id = std::env::var("ROUTER_ID").unwrap_or_else(|_| {
        hostname::get().map_or_else(
            |_| "unknown-router".to_string(),
            |h| h.to_string_lossy().into_owned(),
        )
    });

    // We MUST NOT call init_tracing here because it uses OTLP/Tonic which needs a Tokio reactor.
    // Pingora will provide the reactor once the server starts.
    // We initialize it inside the background services instead.

    info!("Starting Mikrom Router (Pingora)...");

    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let nats_url = std::env::var("NATS_URL").expect("NATS_URL must be set");
    let nats_use_tls = std::env::var("USE_TLS").unwrap_or_default() == "true";
    let nats_certs_dir = std::env::var("NATS_CERTS_DIR")
        .ok()
        .or_else(|| std::env::var("CERTS_DIR").ok());
    let upstream_ca_certs_dir = std::env::var("UPSTREAM_CA_CERTS_DIR").ok();
    let master_key = std::env::var("MASTER_KEY").expect("MASTER_KEY must be set");
    let wg_port = std::env::var("ROUTER_WG_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(51822);
    let advertise_address =
        std::env::var("ROUTER_ADVERTISE_ADDRESS").unwrap_or_else(|_| router_id.clone());
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "/var/lib/mikrom".to_string());
    let cache_path = PathBuf::from(
        std::env::var("STATE_CACHE_PATH")
            .unwrap_or_else(|_| format!("{}/router-state.json", data_dir)),
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

    // 1. Initialize State Manager (Sync)
    let state_manager = Arc::new(state_manager::StateManager::new(cache_path)?);
    let state = state_manager.get_state();

    // 2. Initialize Metrics (Sync)
    let metrics_counters = Arc::new(proxy::RouterMetricsCounters::new());

    // 3. Start Pingora Proxy
    let opt = Opt::default();

    // Configure server settings for enterprise stability
    let conf = ServerConf {
        upgrade_sock: "/tmp/mikrom_router_upgrade.sock".to_string(),
        grace_period_seconds: Some(30), // Allow 30s for active requests to finish
        threads: std::env::var("ROUTER_THREADS")
            .ok()
            .and_then(|s| s.parse().ok())
            .filter(|&n| n > 0)
            .unwrap_or_else(|| {
                std::thread::available_parallelism()
                    .map(std::num::NonZero::get)
                    .unwrap_or(1)
            }),
        ..Default::default()
    };

    let mut server = Server::new_with_opt_and_conf(Some(opt), conf);
    server.bootstrap();

    // 4. Register Background Services
    let cp = control_plane::ControlPlane::new(
        db_url,
        nats_url.clone(),
        nats_use_tls,
        nats_certs_dir.clone(),
        master_key,
        state_manager,
        router_id.clone(),
        advertise_address,
        data_dir,
        wg_port,
    );
    let cp_service = background_service("Control Plane", cp);
    server.add_service(cp_service);

    let telemet = telemetry::TelemetryLoop::new(
        nats_url,
        nats_use_tls,
        nats_certs_dir,
        metrics_counters.clone(),
        router_id,
    );
    let telemet_service = background_service("Telemetry Loop", telemet);
    server.add_service(telemet_service);

    let upstream_ca = upstream_ca::load_upstream_ca(upstream_ca_certs_dir.as_deref())?;
    let proxy_instance = proxy::MikromProxy::new(
        state.clone(),
        acme_staging,
        upstream_ca,
        metrics_counters,
        rps_limit,
    );

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
