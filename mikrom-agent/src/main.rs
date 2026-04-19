use mikrom_agent::server::AgentServer;
use std::net::SocketAddr;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    mikrom_proto::telemetry::init_telemetry("mikrom-agent", env!("CARGO_PKG_VERSION"))?;

    let host_id = std::env::var("HOST_ID").unwrap_or_else(|_| Uuid::new_v4().to_string());

    let scheduler_addr =
        std::env::var("SCHEDULER_ADDR").unwrap_or_else(|_| "http://127.0.0.1:5002".to_string());

    let use_tls = std::env::var("USE_TLS")
        .map(|v| v == "true")
        .unwrap_or(false);

    let agent_port: u16 = std::env::var("AGENT_PORT")
        .unwrap_or_else(|_| "5003".to_string())
        .parse()?;
    let addr: SocketAddr = format!("0.0.0.0:{}", agent_port).parse()?;

    // AGENT_HOSTNAME overrides the machine hostname for registration.
    // Set it to the Docker Compose service name (e.g. "mikrom-agent") so that
    // the scheduler can reach this agent by name when mTLS is enabled — the
    // agent cert has that service name as a SAN.
    let hostname = std::env::var("AGENT_HOSTNAME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            hostname::get()
                .map(|h| h.to_string_lossy().to_string())
                .unwrap_or_else(|_| "unknown".to_string())
        });

    let ip_address = get_local_ip();

    tracing::info!(
        "Starting agent {} on {} (scheduler: {}, hostname: {}, mtls: {})",
        host_id,
        addr,
        scheduler_addr,
        hostname,
        use_tls
    );

    let server = AgentServer::new(host_id, hostname, ip_address);
    server.serve(addr, use_tls).await?;

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
