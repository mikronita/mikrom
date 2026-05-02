use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use mikrom_router::AppState;
use mikrom_router::nats::start_nats_listener;
use mikrom_router::server::{start_http_server, start_https_server};
use moka::future::Cache;
use sqlx::PgPool;
use tokio_rustls::rustls;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Install the default crypto provider for Rustls 0.23
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let config = mikrom_router::config::Config::from_env().expect("Failed to load config");

    mikrom_proto::telemetry::init_telemetry("mikrom-router", "0.1.0", None)?;

    info!("Connecting to database...");
    let db = PgPool::connect(&config.database_url).await?;

    info!("Running database migrations...");
    sqlx::migrate!("./migrations").run(&db).await?;

    let cache = Cache::builder()
        .max_capacity(1000)
        .time_to_live(std::time::Duration::from_secs(60))
        .build();

    let client = hyper_util::client::legacy::Client::builder(TokioExecutor::new())
        .build(HttpConnector::new());

    let state = AppState {
        db: db.clone(),
        cache,
        client,
    };

    // 1. Start NATS background listener for dynamic updates
    start_nats_listener(config.nats_url, db.clone(), state.cache.clone());

    // 2. Start HTTP server (Redirects to HTTPS + ACME Challenges)
    start_http_server(
        state.clone(),
        config.host.clone(),
        config.http_port,
        config.https_port,
    )
    .await?;

    // 3. Start HTTPS server (Main Proxy)
    start_https_server(
        state,
        db,
        config.host,
        config.https_port,
        config.master_key,
        config.cache_ttl,
    )
    .await?;

    Ok(())
}
