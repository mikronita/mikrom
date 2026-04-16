use std::net::SocketAddr;
use std::sync::Arc;

use mikrom_api::AppState;
use mikrom_api::create_app;
use mikrom_api::db;
use mikrom_api::repositories::postgres_user_repository::PostgresUserRepository;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let db_pool = db::connect().await?;
    db::run_migrations(&db_pool).await?;

    let user_repo = PostgresUserRepository::new(Arc::new(db_pool));
    let state = AppState {
        user_repo: Arc::new(user_repo),
        scheduler_client: None,
    };
    let app = create_app(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 5001));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("Server running on http://{}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}
