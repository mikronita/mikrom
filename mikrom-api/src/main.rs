use std::net::SocketAddr;
use std::sync::Arc;

use mikrom_api::AppState;
use mikrom_api::config::ApiConfig;
use mikrom_api::create_app;
use mikrom_api::db;
use mikrom_api::repositories::postgres_user_repository::PostgresUserRepository;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = ApiConfig::load()?;

    mikrom_proto::telemetry::init_telemetry("mikrom-api", env!("CARGO_PKG_VERSION"))?;

    let db_pool = db::connect(&config.database_url).await?;
    db::run_migrations(&db_pool).await?;

    let user_repo = PostgresUserRepository::new(db_pool.clone());
    let app_repo = mikrom_api::repositories::PostgresAppRepository::new(db_pool.clone());

    let (deployment_events, _) = tokio::sync::broadcast::channel(100);
    let build_semaphore = Arc::new(tokio::sync::Semaphore::new(5)); // Limit to 5 concurrent builds

    let scheduler_config = config.scheduler_config();
    let scheduler_client = match mikrom_api::scheduler::connect(&scheduler_config).await {
        Ok(channel) => {
            tracing::info!(addr = %scheduler_config.addr, "Connected to scheduler");
            Some(mikrom_api::SchedulerClient { channel })
        },
        Err(e) => {
            tracing::warn!(addr = %scheduler_config.addr, "Could not connect to scheduler at startup: {}", e);
            None
        },
    };

    let state = AppState {
        user_repo: Arc::new(user_repo),
        app_repo: Arc::new(app_repo),
        scheduler_client,
        scheduler_config,
        builder_addr: config.builder_addr,
        jwt_secret: config.jwt_secret,
        master_key: config.master_key,
        deployment_events,
        build_semaphore,
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
