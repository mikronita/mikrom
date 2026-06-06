use crate::application::records::DnsRecordStore;
use crate::application::resolution::DnsResolutionService;
use crate::infrastructure::config::DnsConfig;
use crate::infrastructure::dns::MikromDnsHandler;
use crate::infrastructure::metrics;
use crate::infrastructure::upstream::UpstreamDnsForwarder;
use anyhow::Result;
use hickory_server::server::Server;
use std::net::SocketAddr;
use tokio::net::UdpSocket;
use tracing::info;

pub async fn run() -> Result<()> {
    tracing_subscriber::fmt::init();
    info!("Starting mikrom-dns v0.1.0...");

    let config = DnsConfig::from_env()?;
    let store = DnsRecordStore::new();

    for (key, ip) in &config.sys_records {
        store.insert_system(key.clone(), *ip);
    }
    metrics::set_active_records(store.active_records());

    if !config.allowed_subnets.is_empty() {
        info!(?config.allowed_subnets, "ACLs enabled");
    }

    let upstream = if config.upstream_dns.is_empty() {
        None
    } else {
        Some(UpstreamDnsForwarder::connect(&config.upstream_dns, config.upstream_timeout()).await?)
    };

    let nats_store = store.clone();
    let config_for_nats = config.clone();
    tokio::spawn(async move {
        let _ = crate::application::sync::run_nats_subscriber(nats_store, &config_for_nats).await;
    });

    tokio::spawn(async move {
        let addr: SocketAddr = "[::]:9091".parse().expect("metrics address parsing failed");
        let app = axum::Router::new().route(
            "/metrics",
            axum::routing::get(|| async { crate::infrastructure::metrics::render_metrics() }),
        );
        let _ = axum::serve(
            tokio::net::TcpListener::bind(addr)
                .await
                .expect("metrics bind"),
            app,
        )
        .await;
    });

    let resolver = DnsResolutionService::new(
        store.clone(),
        config.upstream_dns.clone(),
        config.allowed_subnets.clone(),
        config.nat64_prefix,
    );
    let handler = MikromDnsHandler::new(resolver, upstream);
    let listen_addr = config.listen_addr;
    let udp_socket = UdpSocket::bind(listen_addr).await?;
    info!(%listen_addr, "mikrom-dns server listening via UDP");

    let mut server = Server::new(handler);
    server.register_socket(udp_socket);
    server.block_until_done().await?;

    Ok(())
}
