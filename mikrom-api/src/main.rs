use std::net::SocketAddr;
use std::sync::Arc;

use mikrom_api::AppState;
use mikrom_api::create_app;
use mikrom_api::db;
use mikrom_api::repositories::postgres_user_repository::PostgresUserRepository;
use mikrom_api::scheduler::SchedulerConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    mikrom_proto::telemetry::init_telemetry("mikrom-api", env!("CARGO_PKG_VERSION"))?;

    let db_pool = db::connect().await?;
    db::run_migrations(&db_pool).await?;

    let user_repo = PostgresUserRepository::new(Arc::new(db_pool));
    let state = AppState {
        user_repo: Arc::new(user_repo),
        scheduler_client: None,
        scheduler_config: SchedulerConfig::from_env(),
        jwt_secret: std::env::var("JWT_SECRET").unwrap_or_else(|_| "secret".to_string()),
        master_key: std::env::var("MASTER_KEY")
            .unwrap_or_else(|_| "default-master-key-change-me-in-production".to_string()),
    };
    let app = create_app(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 5001));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("Server running on http://{}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}
