use mikrom_agent::config::AgentConfig;
use mikrom_agent::server::AgentServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let default_level = if std::env::var("RUST_LOG").is_err() {
        std::env::var("LOG_LEVEL").ok()
    } else {
        None
    };

    let config = AgentConfig::load()?;

    mikrom_proto::telemetry::init_telemetry(
        "mikrom-agent",
        env!("CARGO_PKG_VERSION"),
        default_level.as_deref(),
    )?;

    let hostname = config.hostname();
    let advertise_address = config
        .agent_advertise_address
        .clone()
        .unwrap_or(hostname.clone());

    tracing::info!(
        "Starting agent {} (hostname: {}, mtls: {})",
        config.host_id,
        hostname,
        config.use_tls
    );

    let server = AgentServer::new(config, advertise_address).await;
    let server_for_signal = server.clone();

    // Spawn signal handler for graceful shutdown
    tokio::spawn(async move {
        let mut sigterm =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("Failed to register SIGTERM handler: {e}");
                    return;
                },
            };
        let mut sigint =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("Failed to register SIGINT handler: {e}");
                    return;
                },
            };

        tokio::select! {
            _ = sigterm.recv() => tracing::info!("Received SIGTERM"),
            _ = sigint.recv() => tracing::info!("Received SIGINT"),
        }

        server_for_signal.trigger_shutdown().await;
    });

    server.serve().await?;

    Ok(())
}
