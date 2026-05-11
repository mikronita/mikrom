pub mod control_plane;
pub mod proxy;
pub mod state;
pub mod state_manager;
pub mod telemetry;
pub mod tls;

#[cfg(test)]
mod proxy_tests;

use anyhow::Result;
use pingora::listeners::tls::TlsSettings;
use pingora::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    info!("Starting Mikrom Router (Pingora)...");

    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let nats_url = std::env::var("NATS_URL").expect("NATS_URL must be set");
    let cache_path = PathBuf::from(
        std::env::var("STATE_CACHE_PATH").unwrap_or_else(|_| "router-state.json".to_string()),
    );
    let acme_staging = std::env::var("ACME_STAGING").unwrap_or_default() == "true";
    let router_id = std::env::var("ROUTER_ID").unwrap_or_else(|_| {
        hostname::get().map_or_else(
            |_| "unknown-router".to_string(),
            |h| h.to_string_lossy().into_owned(),
        )
    });

    if acme_staging {
        info!(
            "ACME Staging mode is ENABLED. Certificates will be served from Let's Encrypt Staging (if available in DB)."
        );
    }

    // 1. Initialize State Manager
    let state_manager = Arc::new(state_manager::StateManager::new(cache_path).await?);
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
    let mut server = Server::new(None)?;
    server.bootstrap();

    let proxy_instance = proxy::MikromProxy::new(state.clone(), acme_staging, metrics_counters);

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
