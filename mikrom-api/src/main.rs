use std::net::SocketAddr;
use std::sync::Arc;

use mikrom_api::AppState;
use mikrom_api::config::ApiConfig;
use mikrom_api::create_app;
use mikrom_api::db;
use mikrom_api::repositories::postgres_user_repository::PostgresUserRepository;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Install the default crypto provider for Rustls 0.23
    let _ = rustls::crypto::ring::default_provider().install_default();

    let config = ApiConfig::load()?;

    mikrom_proto::telemetry::init_telemetry("mikrom-api", env!("CARGO_PKG_VERSION"))?;

    let db_pool = db::connect(&config.database_url).await?;
    db::run_migrations(&db_pool).await?;

    let user_repo = PostgresUserRepository::new(db_pool.clone());
    let app_repo = mikrom_api::repositories::PostgresAppRepository::new(
        db_pool.clone(),
        config.master_key.clone(),
    );

    let (deployment_events, _) = tokio::sync::broadcast::channel(100);

    tracing::info!("Connecting to NATS at {}...", config.nats_url);
    let nats_client = async_nats::connect(&config.nats_url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to NATS: {}", e))?;

    let scheduler = Arc::new(mikrom_api::scheduler::NatsScheduler {
        client: nats_client.clone(),
    });

    let state = AppState {
        user_repo: Arc::new(user_repo),
        app_repo: Arc::new(app_repo),
        scheduler,
        nats_client,
        router_addr: config.router_addr,
        api_db: db_pool,
        jwt_secret: config.jwt_secret,
        master_key: config.master_key,
        deployment_events: deployment_events.clone(),
        acme_email: config.acme_email,
        acme_staging: config.acme_staging,
        acme_check_interval: config.acme_check_interval,
    };

    mikrom_api::start_background_tasks(state.clone());

    let app = create_app(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], config.api_port));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("Server running on http://{}", addr);

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
