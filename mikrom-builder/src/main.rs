mod builder;
mod config;
mod server;
mod state;

use tracing::{error, info};

use crate::builder::AppBuilder;
use crate::config::Config;
use crate::server::BuilderServer;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config =
        Config::from_env().map_err(|e| anyhow::anyhow!("Failed to load configuration: {e}"))?;

    let _telemetry = mikrom_proto::telemetry::init_telemetry(
        "mikrom-builder",
        env!("CARGO_PKG_VERSION"),
        Some(&config.log_level),
    )?;
    mikrom_proto::telemetry::record_service_startup("mikrom-builder");

    info!("Connecting to NATS at {}...", config.nats_url);
    let nats_client = async_nats::connect(&config.nats_url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to NATS: {}", e))?;

    info!("mikrom-builder started");

    let builder = AppBuilder::new(config.registry, config.registry_user, config.registry_pass);
    let builder_server = BuilderServer::new(
        builder,
        config.max_concurrent_builds,
        std::time::Duration::from_secs(config.build_state_ttl_secs),
        config.build_state_path,
    )
    .await?;

    let shutdown = CancellationToken::new();
    let shutdown_for_signal = shutdown.clone();

    tokio::spawn(async move {
        let mut sigterm =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(signal) => signal,
                Err(e) => {
                    error!("Failed to register SIGTERM handler: {}", e);
                    return;
                },
            };
        let mut sigint =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()) {
                Ok(signal) => signal,
                Err(e) => {
                    error!("Failed to register SIGINT handler: {}", e);
                    return;
                },
            };

        tokio::select! {
            _ = sigterm.recv() => info!("Received SIGTERM"),
            _ = sigint.recv() => info!("Received SIGINT"),
        }

        shutdown_for_signal.cancel();
    });

    if let Err(e) = builder_server.listen(nats_client, shutdown).await {
        error!("Builder server listener failed: {}", e);
    }

    Ok(())
}
