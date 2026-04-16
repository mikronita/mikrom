use mikrom_scheduler::server::SchedulerServer;
use std::net::SocketAddr;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

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
