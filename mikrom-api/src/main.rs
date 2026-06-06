use std::net::{Ipv6Addr, SocketAddr, TcpListener};
use std::sync::Arc;

use mikrom_api::AppState;
use mikrom_api::application::ApiContext;
use mikrom_api::config::ApiConfig;
use mikrom_api::create_app_with_rate_limits;
use mikrom_api::infrastructure::db;
use mikrom_api::infrastructure::db::{
    PostgresAppRepository, PostgresDatabaseRepository, PostgresGithubRepository,
    PostgresTenantRepository, PostgresUserRepository, PostgresVolumeRepository,
};

fn bind_ipv6_dual_stack_listener(port: u16) -> anyhow::Result<TcpListener> {
    let socket = socket2::Socket::new(
        socket2::Domain::IPV6,
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )
    .map_err(|e| anyhow::anyhow!("failed to create IPv6 listener socket on port {port}: {e}"))?;
    socket
        .set_reuse_address(true)
        .map_err(|e| anyhow::anyhow!("failed to set SO_REUSEADDR on port {port}: {e}"))?;
    socket
        .set_only_v6(false)
        .map_err(|e| anyhow::anyhow!("failed to enable dual-stack mode on port {port}: {e}"))?;

    let addr = std::net::SocketAddr::from((Ipv6Addr::UNSPECIFIED, port));
    socket
        .bind(&socket2::SockAddr::from(addr))
        .map_err(|e| anyhow::anyhow!("failed to bind dual-stack listener on {addr}: {e}"))?;
    socket
        .listen(1024)
        .map_err(|e| anyhow::anyhow!("failed to listen on {addr}: {e}"))?;
    socket
        .set_nonblocking(true)
        .map_err(|e| anyhow::anyhow!("failed to set nonblocking on {addr}: {e}"))?;

    Ok(socket.into())
}

fn bind_dual_stack_listener(port: u16) -> anyhow::Result<TcpListener> {
    bind_ipv6_dual_stack_listener(port)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Install the default crypto provider for Rustls 0.23
    let _ = rustls::crypto::ring::default_provider().install_default();

    let config = ApiConfig::load()?;

    let _telemetry =
        mikrom_proto::telemetry::init_telemetry("mikrom-api", env!("CARGO_PKG_VERSION"), None)?;
    mikrom_proto::telemetry::record_service_startup("mikrom-api");

    let db_pool = db::connect(&config.database_url).await?;
    db::run_migrations(&db_pool).await?;

    let rate_limit_config = mikrom_api::rate_limit::RateLimitConfig::from_api_config(&config)?;
    let jwt_secret = config.jwt_secret.clone();
    let api_port = config.api_port;

    let user_repo = Arc::new(PostgresUserRepository::new(db_pool.clone()));
    let tenant_repo = Arc::new(PostgresTenantRepository::new(db_pool.clone()));
    let app_repo = Arc::new(PostgresAppRepository::new(
        db_pool.clone(),
        config.master_key.clone(),
    ));
    let database_repo = Arc::new(PostgresDatabaseRepository::new(db_pool.clone()));
    let github_repo = Arc::new(PostgresGithubRepository::new(db_pool.clone()));
    let volume_repo = Arc::new(PostgresVolumeRepository::new(db_pool.clone()));

    tracing::info!("Connecting to NATS at {}...", config.nats_url);
    // Disable the client's built-in request timeout so the application-level
    // `TypedNatsClient` timeouts are the only ones that apply to request/reply flows.
    let nats_client = async_nats::connect_with_options(
        &config.nats_url,
        async_nats::ConnectOptions::new().request_timeout(None),
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to connect to NATS: {}", e))?;
    let nats = mikrom_api::nats::TypedNatsClient::new(nats_client.clone()).with_timeout(
        std::time::Duration::from_secs(config.nats_request_timeout_secs.max(1)),
    );

    let scheduler = Arc::new(mikrom_api::NatsScheduler::new(
        nats.clone(),
        std::time::Duration::from_secs(config.nats_scheduler_long_timeout_secs.max(1)),
        std::time::Duration::from_secs(config.nats_scheduler_database_timeout_secs.max(1)),
    ));

    let ctx = ApiContext::new(
        user_repo.clone(),
        tenant_repo.clone(),
        app_repo.clone(),
        database_repo.clone(),
        github_repo.clone(),
        volume_repo.clone(),
        scheduler.clone(),
        nats.clone(),
        db_pool.clone(),
        config.clone(),
    );

    let (deployment_events, _) = tokio::sync::broadcast::channel(100);
    let (workspace_events, _) = tokio::sync::broadcast::channel(100);
    let (mesh_status, _) =
        tokio::sync::watch::channel(mikrom_api::domain::worker::MeshStatus::default());

    let state = AppState {
        ctx: ctx.clone(),
        user_repo,
        tenant_repo,
        app_repo,
        database_repo,
        github_repo,
        volume_repo,
        scheduler,
        nats,
        router_addr: config.router_addr,
        frontend_url: config.frontend_url,
        api_db: db_pool,
        jwt_secret: config.jwt_secret,
        master_key: config.master_key,
        deployment_events: deployment_events.clone(),
        workspace_events: workspace_events.clone(),
        mesh_status: mesh_status.clone(),
        acme_email: config.acme_email,
        acme_staging: config.acme_staging,
        acme_check_interval: config.acme_check_interval,
        github_app_id: config.github_app_id,
        github_private_key: config.github_private_key,
        github_app_slug: config.github_app_slug,
        github_webhook_url_base: config.github_webhook_url_base,
        active_deployment_flows: Arc::new(dashmap::DashSet::new()),
    };

    mikrom_api::application::vms::prime_mesh_status_cache(&state).await?;
    mikrom_api::start_background_tasks(state.clone());

    let rate_limiter = Arc::new(mikrom_api::rate_limit::RateLimiter::new(
        rate_limit_config,
        jwt_secret,
    )?);

    let app = create_app_with_rate_limits(state, rate_limiter);

    let listener =
        tokio::net::TcpListener::from_std(bind_dual_stack_listener(api_port)?).map_err(|e| {
            anyhow::anyhow!("failed to convert listener on port {api_port} to tokio: {e}")
        })?;
    let addr = listener.local_addr()?;

    tracing::info!(listen_addr = %addr, "Server running on http://{addr}");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::bind_dual_stack_listener;
    use std::net::{Ipv6Addr, SocketAddr, TcpStream};

    #[test]
    fn dual_stack_listener_accepts_ipv6() {
        let listener = match bind_dual_stack_listener(0) {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!("skipping api smoke test: dual-stack bind unavailable: {err}");
                return;
            },
        };
        let local_addr = listener
            .local_addr()
            .expect("local addr should be available");
        let port = local_addr.port();

        if !local_addr.is_ipv6() {
            eprintln!("skipping api smoke test: listener did not bind to ipv6");
            return;
        }

        let v6 = SocketAddr::from((Ipv6Addr::LOCALHOST, port));
        let stream = match TcpStream::connect(v6) {
            Ok(stream) => stream,
            Err(err) => {
                eprintln!("skipping api smoke test: ipv6 loopback unavailable: {err}");
                return;
            },
        };

        let handle = std::thread::spawn(move || {
            let _ = listener.accept();
        });

        handle.join().expect("listener thread should exit");
        drop(stream);
    }
}
