use crate::app::config::RouterConfig;
use crate::app::runtime;
use crate::application::{control_plane, proxy, telemetry, traffic};
use crate::domain::health::RouterHealth;
use crate::infrastructure::persistence::state_manager;
use crate::infrastructure::{tls, upstream_ca};
use anyhow::Result;
use pingora::listeners::{TcpSocketOptions, tls::TlsSettings};
use pingora::prelude::*;
use pingora::server::RunArgs;
use std::net::Ipv6Addr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

fn dual_stack_tcp_socket_options() -> TcpSocketOptions {
    let mut options = TcpSocketOptions::default();
    options.ipv6_only = Some(false);
    options
}

fn ipv6_supported() -> bool {
    std::net::TcpListener::bind((Ipv6Addr::UNSPECIFIED, 0)).is_ok()
}

pub fn run(config: &RouterConfig) -> Result<()> {
    runtime::init_bootstrap_tracing_once();
    runtime::init_tracing_once(config.router_id.as_str());
    mikrom_proto::telemetry::record_service_startup("mikrom-router");
    info!("Starting Mikrom Router (Pingora)...");
    let health = Arc::new(RouterHealth::new());
    health.mark_bootstrapped();

    if config.acme_staging {
        info!(
            "ACME Staging mode is ENABLED. Certificates will be served from Let's Encrypt Staging (if available in DB)."
        );
    }

    let state_manager = Arc::new(state_manager::StateManager::new(
        config.state_cache_path().clone(),
    )?);
    let state = state_manager.get_state();
    let metrics_counters = Arc::new(proxy::RouterMetricsCounters::new());

    let (traffic_tx, traffic_rx) = mpsc::channel(1024);
    let traffic_publisher = Arc::new(traffic::RouterTrafficPublisher::new(
        config.router_id.clone(),
        traffic_tx,
    ));

    let mut server = Server::new_with_opt_and_conf(
        Some(Opt::default()),
        runtime::server_conf(config.router_threads),
    );
    server.bootstrap();

    let cp = control_plane::ControlPlane::new(
        config.database_url.clone(),
        config.nats_url.clone(),
        config.nats_use_tls,
        config.nats_certs_dir.clone(),
        config.master_key.clone(),
        state_manager,
        health.clone(),
        config.router_id.clone(),
        config.advertise_address().to_string(),
        config.data_dir.to_string_lossy().into_owned(),
        config.wireguard_port,
    );
    server.add_service(background_service("Control Plane", cp));

    let telemetry_loop = telemetry::TelemetryLoop::new(
        metrics_counters.clone(),
        health.clone(),
        state.clone(),
        config.router_id.clone(),
    );
    server.add_service(background_service("Telemetry Loop", telemetry_loop));

    let traffic_loop = traffic::RouterTrafficLoop::new(
        config.nats_url.clone(),
        config.nats_use_tls,
        config.nats_certs_dir.clone(),
        traffic_rx,
    );
    server.add_service(background_service("Router Traffic Loop", traffic_loop));

    let upstream_ca = upstream_ca::load_upstream_ca(config.upstream_ca_certs_dir.as_deref())?;
    health.mark_upstream_ca_ready();
    let proxy_instance = proxy::MikromProxy::new(
        state.clone(),
        health,
        config.acme_staging,
        config.default_site_host.clone(),
        config.default_site_redirect_url.clone(),
        upstream_ca,
        metrics_counters,
        Some(traffic_publisher),
        config.rps_limit,
    );

    let use_ipv6 = ipv6_supported();
    let listen_host = if use_ipv6 { "[::]" } else { "0.0.0.0" };
    let listen_tcp = format!("{listen_host}:80");
    let listen_tls = format!("{listen_host}:443");

    let mut proxy_service = http_proxy_service(&server.configuration, proxy_instance);
    if use_ipv6 {
        proxy_service.add_tcp_with_settings(&listen_tcp, dual_stack_tcp_socket_options());
    } else {
        proxy_service.add_tcp(&listen_tcp);
    }

    let tls_handler = tls::MikromTlsHandler::new(state);
    let mut tls_settings = TlsSettings::with_callbacks(Box::new(tls_handler))?;
    tls_settings.enable_h2();
    if use_ipv6 {
        proxy_service.add_tls_with_settings(
            &listen_tls,
            Some(dual_stack_tcp_socket_options()),
            tls_settings,
        );
    } else {
        proxy_service.add_tls_with_settings(&listen_tls, None, tls_settings);
    }

    server.add_service(proxy_service);
    server.run(RunArgs::default());
    runtime::shutdown_telemetry();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::dual_stack_tcp_socket_options;

    #[test]
    fn dual_stack_listener_options_disable_ipv6_only() {
        let options = dual_stack_tcp_socket_options();
        assert_eq!(options.ipv6_only, Some(false));
    }

    #[test]
    fn ipv6_support_probe_succeeds_or_falls_back() {
        let _ = super::ipv6_supported();
    }
}
