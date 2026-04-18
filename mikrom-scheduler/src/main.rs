use mikrom_scheduler::server::SchedulerServer;
use std::net::SocketAddr;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    if std::env::var("LOG_FORMAT").unwrap_or_default() == "json" {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }

    let use_tls = std::env::var("USE_TLS")
        .map(|v| v == "true")
        .unwrap_or(false);

    let certs = if use_tls {
        let certs_dir =
            std::env::var("CERTS_DIR").unwrap_or_else(|_| "/certs/scheduler".to_string());
        tracing::info!("Loading TLS certificates from {}", certs_dir);
        Some(mikrom_proto::tls::ServiceCerts::load(&certs_dir)?)
    } else {
        None
    };

    let addr: SocketAddr = "0.0.0.0:5002".parse()?;
    tracing::info!("Starting scheduler on {} (mtls: {})", addr, use_tls);

    let server = SchedulerServer::new(certs)?;
    server.serve(addr).await?;

    Ok(())
}
