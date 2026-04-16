use std::net::SocketAddr;
use tracing_subscriber;

use mikrom_api::AppState;
use mikrom_api::create_app;
use mikrom_api::db;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let db_pool = db::connect().await?;
    db::run_migrations(&db_pool).await?;

    let state = AppState {
        db: db_pool,
        scheduler_client: None,
    };
    let app = create_app(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 5001));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("Server running on http://{}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}
