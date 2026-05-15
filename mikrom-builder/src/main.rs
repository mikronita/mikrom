mod builder;
mod config;
mod server;

use tracing::{error, info};

use crate::builder::AppBuilder;
use crate::config::Config;
use crate::server::BuilderServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env().expect("Failed to load configuration");

    mikrom_proto::telemetry::init_telemetry("mikrom-builder", "0.1.0", Some(&config.log_level))?;

    info!("Connecting to NATS at {}...", config.nats_url);
    let nats_client = async_nats::connect(&config.nats_url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to NATS: {}", e))?;

    info!("mikrom-builder started");

    let builder = AppBuilder::new(
        config.registry,
        config.buildpack_builder,
        config.registry_user,
        config.registry_pass,
    );
    let builder_server = BuilderServer::new(builder);

    if let Err(e) = builder_server.listen(nats_client).await {
        error!("Builder server listener failed: {}", e);
    }

    Ok(())
}
