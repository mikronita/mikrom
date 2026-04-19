use mikrom_scheduler::server::SchedulerServer;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    mikrom_proto::telemetry::init_telemetry("mikrom-scheduler", env!("CARGO_PKG_VERSION"))?;

    let use_tls = std::env::var("USE_TLS").unwrap_or_default() == "true";

    let _scheduler_port: u16 = std::env::var("SCHEDULER_PORT")
        .unwrap_or_else(|_| "5002".to_string())
        .parse()?;

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
