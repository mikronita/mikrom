use mikrom_scheduler::application::{AppService, SchedulerRuntimeConfig};
use mikrom_scheduler::config::SchedulerConfig;
use mikrom_scheduler::infrastructure::db::{PgAppRepository, PgJobRepository, PgWorkerRepository};
use mikrom_scheduler::infrastructure::http::SchedulerHttpServer;
use mikrom_scheduler::infrastructure::nats::{NatsAgentClient, NatsEventLoop};
use mikrom_scheduler::server::SchedulerServer;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = SchedulerConfig::load()?;

    mikrom_proto::telemetry::init_telemetry("mikrom-scheduler", env!("CARGO_PKG_VERSION"), None)?;

    tracing::info!("Connecting to database...");
    let database_max_connections = config.database_max_connections.max(1);
    let pool = PgPoolOptions::new()
        .max_connections(database_max_connections)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&config.database_url)
        .await?;

    tracing::info!("Running database migrations...");
    sqlx::migrate!("./migrations").run(&pool).await?;

    let certs = if config.use_tls {
        tracing::info!("Loading TLS certificates from {}", config.certs_dir);
        Some(mikrom_proto::tls::ServiceCerts::load(&config.certs_dir)?)
    } else {
        None
    };

    tracing::info!("Connecting to NATS at {}...", config.nats_url);
    let nats_client = async_nats::connect(&config.nats_url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to NATS: {}", e))?;

    // Dependency Injection
    let job_repo = Arc::new(PgJobRepository::new(pool.clone()));
    let app_repo = Arc::new(PgAppRepository::new(pool.clone()));
    let worker_repo = Arc::new(PgWorkerRepository::new(pool.clone()));
    let agent_client = Arc::new(NatsAgentClient::new(nats_client.clone()));

    let runtime = SchedulerRuntimeConfig {
        router_idle_timeout_secs: config.router_idle_timeout_secs,
        worker_stale_threshold_secs: config.worker_stale_threshold_secs,
        restore_retry_backoff_secs: config.restore_retry_backoff_secs,
    };

    let app_service = Arc::new(AppService::new(
        job_repo,
        app_repo,
        worker_repo,
        agent_client,
        nats_client.clone(),
        pool.clone(),
        runtime,
    ));

    let server = SchedulerServer::new(app_service.clone(), certs);
    let observability_server = SchedulerHttpServer::new(
        config.http_port,
        pool.clone(),
        nats_client.clone(),
        app_service.context.telemetry.clone(),
        config.worker_stale_threshold_secs,
        config.database_max_connections,
    );

    // Periodic pool telemetry for diagnosing contention and starvation.
    let pool_for_metrics = pool.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            tracing::info!(
                db_pool_size = pool_for_metrics.size(),
                db_pool_idle = pool_for_metrics.num_idle(),
                db_pool_max_connections = database_max_connections,
                db_pool_closed = pool_for_metrics.is_closed(),
                "Scheduler database pool snapshot"
            );
        }
    });

    // Expose health and metrics for Prometheus-style scraping.
    observability_server.spawn();

    // Start background cleanup task
    let app_service_for_cleanup = app_service.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;

            match app_service_for_cleanup.cleanup_stale_workers().await {
                Ok(count) => {
                    if count > 0 {
                        tracing::info!("Marked {} stale workers as Offline", count);
                    }
                },
                Err(e) => tracing::error!("Failed to cleanup stale workers: {}", e),
            }
        }
    });

    // Start autoscaler
    let app_service_clone = app_service.clone();
    tokio::spawn(async move {
        app_service_clone.start_autoscaler().await;
    });

    // Start NATS event loop
    let event_loop = NatsEventLoop::new(server, nats_client);
    event_loop.run().await?;

    Ok(())
}
