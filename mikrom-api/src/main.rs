use std::net::SocketAddr;
use std::sync::Arc;

use mikrom_api::AppState;
use mikrom_api::application::ApiContext;
use mikrom_api::config::ApiConfig;
use mikrom_api::create_app_with_rate_limits;
use mikrom_api::infrastructure::db;
use mikrom_api::infrastructure::db::{
    PostgresAppRepository, PostgresGithubRepository, PostgresUserRepository,
    PostgresVolumeRepository,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Install the default crypto provider for Rustls 0.23
    let _ = rustls::crypto::ring::default_provider().install_default();

    let config = ApiConfig::load()?;

    mikrom_proto::telemetry::init_telemetry("mikrom-api", env!("CARGO_PKG_VERSION"), None)?;

    let db_pool = db::connect(&config.database_url).await?;
    db::run_migrations(&db_pool).await?;

    let rate_limit_config = mikrom_api::rate_limit::RateLimitConfig::from_api_config(&config)?;
    let jwt_secret = config.jwt_secret.clone();
    let api_port = config.api_port;

    let user_repo = Arc::new(PostgresUserRepository::new(db_pool.clone()));
    let app_repo = Arc::new(PostgresAppRepository::new(
        db_pool.clone(),
        config.master_key.clone(),
    ));
    let github_repo = Arc::new(PostgresGithubRepository::new(db_pool.clone()));
    let volume_repo = Arc::new(PostgresVolumeRepository::new(db_pool.clone()));

    tracing::info!("Connecting to NATS at {}...", config.nats_url);
    let nats_client = async_nats::connect(&config.nats_url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to NATS: {}", e))?;
    let nats = mikrom_api::nats::TypedNatsClient::new(nats_client.clone());

    let scheduler = Arc::new(mikrom_api::NatsScheduler::new(nats.clone()));

    let ctx = ApiContext::new(
        user_repo.clone(),
        app_repo.clone(),
        github_repo.clone(),
        volume_repo.clone(),
        scheduler.clone(),
        nats.clone(),
        db_pool.clone(),
        config.clone(),
    );

    let (deployment_events, _) = tokio::sync::broadcast::channel(100);
    let (workspace_events, _) = tokio::sync::broadcast::channel(100);
    let (mesh_status, _) =
        tokio::sync::watch::channel(mikrom_api::domain::worker::MeshStatus::default());

    let state = AppState {
        ctx: ctx.clone(),
        user_repo,
        app_repo,
        github_repo,
        volume_repo,
        scheduler,
        nats,
        router_addr: config.router_addr,
        frontend_url: config.frontend_url,
        api_db: db_pool,
        jwt_secret: config.jwt_secret,
        master_key: config.master_key,
        deployment_events: deployment_events.clone(),
        workspace_events: workspace_events.clone(),
        mesh_status: mesh_status.clone(),
        acme_email: config.acme_email,
        acme_staging: config.acme_staging,
        acme_check_interval: config.acme_check_interval,
        github_app_id: config.github_app_id,
        github_private_key: config.github_private_key,
        github_app_slug: config.github_app_slug,
        github_webhook_url_base: config.github_webhook_url_base,
        active_deployment_flows: Arc::new(dashmap::DashSet::new()),
    };

    mikrom_api::application::vms::prime_mesh_status_cache(&state).await?;
    mikrom_api::start_background_tasks(state.clone());

    let rate_limiter = Arc::new(mikrom_api::rate_limit::RateLimiter::new(
        rate_limit_config,
        jwt_secret,
    )?);

    let app = create_app_with_rate_limits(state, rate_limiter);

    let addr = SocketAddr::from(([0, 0, 0, 0], api_port));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("Server running on http://{}", addr);

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
