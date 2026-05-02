use mikrom_agent::config::AgentConfig;
use mikrom_agent::server::AgentServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = AgentConfig::load()?;

    mikrom_proto::telemetry::init_telemetry("mikrom-agent", env!("CARGO_PKG_VERSION"), None)?;

    let hostname = config.hostname();
    let ip_address = get_local_ip();

    tracing::info!(
        "Starting agent {} (hostname: {}, mtls: {})",
        config.host_id,
        hostname,
        config.use_tls
    );

    let server = AgentServer::new(config, ip_address);
    server.serve().await?;

    Ok(())
}

fn get_local_ip() -> String {
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0")
        && socket.connect("8.8.8.8:80").is_ok()
        && let Ok(addr) = socket.local_addr()
        && let std::net::SocketAddr::V4(v4) = addr
    {
        return v4.ip().to_string();
    }
    "127.0.0.1".to_string()
}
