mod builder;
mod config;
mod server;

use std::net::SocketAddr;
use tonic::transport::Server;
use tracing::info;

use crate::builder::AppBuilder;
use crate::config::Config;
use crate::server::BuilderServer;
use mikrom_proto::builder::builder_service_server::BuilderServiceServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env().expect("Failed to load configuration");

    mikrom_proto::telemetry::init_telemetry("mikrom-builder", "0.1.0")?;

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
