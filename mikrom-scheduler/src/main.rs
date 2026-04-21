use mikrom_scheduler::config::SchedulerConfig;
use mikrom_scheduler::server::SchedulerServer;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = SchedulerConfig::load()?;

    mikrom_proto::telemetry::init_telemetry("mikrom-scheduler", env!("CARGO_PKG_VERSION"))?;

    let certs = if config.use_tls {
        tracing::info!("Loading TLS certificates from {}", config.certs_dir);
        Some(mikrom_proto::tls::ServiceCerts::load(&config.certs_dir)?)
    } else {
        None
    };

    let addr: SocketAddr = format!("0.0.0.0:{}", config.scheduler_port).parse()?;
    tracing::info!("Starting scheduler on {} (mtls: {})", addr, config.use_tls);

    let server = SchedulerServer::new(certs)?;
    server.serve(addr).await?;

    Ok(())
}
