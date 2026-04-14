use mikrom_scheduler::server::SchedulerServer;
use std::net::SocketAddr;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("info"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let use_tls = std::env::var("USE_TLS")
        .map(|v| v == "true")
        .unwrap_or(false);

    let addr: SocketAddr = "0.0.0.0:5002".parse()?;
    tracing::info!("Starting scheduler gRPC server on {} (tls: {})", addr, use_tls);

    let server = SchedulerServer::new(addr)?;
    server.serve(use_tls).await?;

    Ok(())
}