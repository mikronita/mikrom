mod builder;
mod config;
mod server;

use std::net::SocketAddr;
use tonic::transport::Server;
use tracing::{Level, info};
use tracing_subscriber::FmtSubscriber;

use crate::builder::AppBuilder;
use crate::config::Config;
use crate::server::BuilderServer;
use mikrom_proto::builder::builder_service_server::BuilderServiceServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env().expect("Failed to load configuration");

    let log_level = match config.log_level.to_lowercase().as_str() {
        "debug" => Level::DEBUG,
        "info" => Level::INFO,
        "warn" => Level::WARN,
        "error" => Level::ERROR,
        _ => Level::INFO,
    };

    let subscriber = FmtSubscriber::builder().with_max_level(log_level).finish();

    tracing::subscriber::set_global_default(subscriber).expect("Setting default subscriber failed");

    let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;

    info!(addr = %addr, "Starting mikrom-builder gRPC server");

    let builder = AppBuilder::new(config.registry, config.buildpack_builder);
    let builder_server = BuilderServer::new(builder);

    Server::builder()
        .add_service(BuilderServiceServer::new(builder_server))
        .serve(addr)
        .await?;

    Ok(())
}
