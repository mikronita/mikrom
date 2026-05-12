use mikrom_agent::config::AgentConfig;
use mikrom_agent::server::AgentServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    #[allow(clippy::collapsible_if)]
    if std::env::var("RUST_LOG").is_err() {
        if let Ok(level) = std::env::var("LOG_LEVEL") {
            unsafe {
                std::env::set_var("RUST_LOG", level);
            }
        }
    }

    let config = AgentConfig::load()?;

    mikrom_proto::telemetry::init_telemetry("mikrom-agent", env!("CARGO_PKG_VERSION"), None)?;

    let hostname = config.hostname();
    let advertise_address = hostname.clone();

    tracing::info!(
        "Starting agent {} (hostname: {}, mtls: {})",
        config.host_id,
        hostname,
        config.use_tls
    );

    let server = AgentServer::new(config, advertise_address).await;
    server.serve().await?;

    Ok(())
}
