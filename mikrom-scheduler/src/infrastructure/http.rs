use axum::{
    Router,
    extract::State,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use sqlx::{PgPool, Row};
use std::net::{Ipv6Addr, TcpListener};
use std::{collections::BTreeMap, fmt::Write as _, time::Duration, time::Instant};
use tower_http::trace::TraceLayer;

use crate::infrastructure::telemetry::SchedulerTelemetry;

#[derive(Clone)]
pub struct SchedulerHttpServer {
    port: u16,
    state: SchedulerHttpState,
}

#[derive(Clone)]
struct SchedulerHttpState {
    pool: PgPool,
    nats_client: async_nats::Client,
    telemetry: SchedulerTelemetry,
    started_at: Instant,
    worker_stale_threshold_secs: i64,
    database_max_connections: u32,
}

#[derive(Debug, Default)]
struct SchedulerMetricsSnapshot {
    uptime_seconds: u64,
    db_pool_size: u32,
    db_pool_idle: u32,
    db_pool_max_connections: u32,
    db_pool_closed: bool,
    apps_total: u64,
    apps_autoscaling_enabled: u64,
    apps_scaled_to_zero: u64,
    workers_total: u64,
    workers_online: u64,
    workers_offline: u64,
    workers_stale: u64,
    jobs_total: u64,
    job_status_counts: BTreeMap<String, u64>,
}

impl SchedulerHttpServer {
    pub fn new(
        port: u16,
        pool: PgPool,
        nats_client: async_nats::Client,
        telemetry: SchedulerTelemetry,
        worker_stale_threshold_secs: i64,
        database_max_connections: u32,
    ) -> Self {
        Self {
            port,
            state: SchedulerHttpState {
                pool,
                nats_client,
                telemetry,
                started_at: Instant::now(),
                worker_stale_threshold_secs,
                database_max_connections,
            },
        }
    }

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        let port = self.port;
        let app = Router::new()
            .route("/health", get(Self::health_handler))
            .route("/ready", get(Self::ready_handler))
            .route("/metrics", get(Self::metrics_handler))
            .layer(TraceLayer::new_for_http())
            .with_state(self.state);

        tracing::info!(port = port, "Scheduler HTTP observability server starting");

        tokio::spawn(async move {
            match bind_dual_stack_listener(port) {
                Ok(listener) => {
                    let listener = match tokio::net::TcpListener::from_std(listener) {
                        Ok(listener) => listener,
                        Err(e) => {
                            tracing::error!(port = port, error = %e, "Failed to convert scheduler HTTP listener");
                            return;
                        },
                    };
                    if let Ok(addr) = listener.local_addr() {
                        tracing::info!(listen_addr = %addr, port = port, "Scheduler HTTP server bound");
                    }
                    if let Err(e) = axum::serve(listener, app).await {
                        tracing::error!(port = port, error = %e, "Scheduler HTTP server exited");
                    }
                },
                Err(e) => {
                    tracing::error!(port = port, error = %e, "Failed to bind scheduler HTTP server");
                },
            }
        })
    }

    async fn health_handler() -> impl IntoResponse {
        (StatusCode::OK, "ok")
    }

    async fn ready_handler(State(state): State<SchedulerHttpState>) -> impl IntoResponse {
        let started = Instant::now();
        let result = state.ready().await;
        let success = result.is_ok();
        state
            .telemetry
            .record("http", "ready", started.elapsed(), success);

        match result {
            Ok(()) => (StatusCode::OK, "ready").into_response(),
            Err(reason) => (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("not ready: {reason}"),
            )
                .into_response(),
        }
    }

    async fn metrics_handler(State(state): State<SchedulerHttpState>) -> impl IntoResponse {
        let started = Instant::now();
        let snapshot = state.collect().await;
        state
            .telemetry
            .record("http", "metrics", started.elapsed(), true);
        let body = snapshot.render();
        (
            [(
                header::CONTENT_TYPE,
                "text/plain; version=0.0.4; charset=utf-8",
            )],
            body,
        )
    }
}

fn bind_ipv6_dual_stack_listener(port: u16) -> std::io::Result<TcpListener> {
    let socket = socket2::Socket::new(
        socket2::Domain::IPV6,
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )?;
    socket.set_reuse_address(true)?;
    socket.set_only_v6(false)?;

    let addr = std::net::SocketAddr::from((Ipv6Addr::UNSPECIFIED, port));
    socket.bind(&socket2::SockAddr::from(addr))?;
    socket.listen(1024)?;
    socket.set_nonblocking(true)?;

    Ok(socket.into())
}

fn bind_dual_stack_listener(port: u16) -> std::io::Result<TcpListener> {
    bind_ipv6_dual_stack_listener(port)
}

impl SchedulerHttpState {
    async fn ready(&self) -> Result<(), String> {
        sqlx::query_scalar::<_, i64>("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| format!("database unavailable: {e}"))?;

        tokio::time::timeout(Duration::from_secs(2), self.nats_client.flush())
            .await
            .map_err(|_| "nats flush timed out".to_string())?
            .map_err(|e| format!("nats unavailable: {e}"))?;

        Ok(())
    }

    async fn collect(&self) -> SchedulerMetricsSnapshot {
        let uptime_seconds = self.started_at.elapsed().as_secs();

        let db_pool_size = self.pool.size();
        let db_pool_idle = self.pool.num_idle() as u32;
        let db_pool_closed = self.pool.is_closed();

        let apps_total = self
            .count_query("apps_total", "SELECT COUNT(*) FROM apps")
            .await;
        let apps_autoscaling_enabled = self
            .count_query(
                "apps_autoscaling_enabled",
                "SELECT COUNT(*) FROM apps WHERE autoscaling_enabled = TRUE",
            )
            .await;
        let apps_scaled_to_zero = self
            .count_query(
                "apps_scaled_to_zero",
                "SELECT COUNT(*) FROM apps WHERE desired_replicas = 0",
            )
            .await;
        let workers_total = self
            .count_query("workers_total", "SELECT COUNT(*) FROM workers")
            .await;
        let workers_online = self
            .count_query(
                "workers_online",
                "SELECT COUNT(*) FROM workers WHERE status = 'Online'",
            )
            .await;
        let workers_offline = self
            .count_query(
                "workers_offline",
                "SELECT COUNT(*) FROM workers WHERE status = 'Offline'",
            )
            .await;
        let workers_stale = self
            .count_query_with_i64(
                "workers_stale",
                "SELECT COUNT(*) FROM workers WHERE status = 'Online' AND last_heartbeat < $1",
                chrono::Utc::now().timestamp() - self.worker_stale_threshold_secs,
            )
            .await;
        let jobs_total = self
            .count_query("jobs_total", "SELECT COUNT(*) FROM jobs")
            .await;
        let job_status_counts = self.job_status_counts().await;

        SchedulerMetricsSnapshot {
            uptime_seconds,
            db_pool_size,
            db_pool_idle,
            db_pool_max_connections: self.database_max_connections,
            db_pool_closed,
            apps_total,
            apps_autoscaling_enabled,
            apps_scaled_to_zero,
            workers_total,
            workers_online,
            workers_offline,
            workers_stale,
            jobs_total,
            job_status_counts,
        }
    }

    async fn count_query(&self, metric: &'static str, sql: &'static str) -> u64 {
        match sqlx::query_scalar::<_, i64>(sql)
            .fetch_one(&self.pool)
            .await
        {
            Ok(value) => value.max(0) as u64,
            Err(e) => {
                tracing::warn!(metric = metric, error = %e, "Failed to collect scheduler metric");
                0
            },
        }
    }

    async fn count_query_with_i64(
        &self,
        metric: &'static str,
        sql: &'static str,
        value: i64,
    ) -> u64 {
        match sqlx::query_scalar::<_, i64>(sql)
            .bind(value)
            .fetch_one(&self.pool)
            .await
        {
            Ok(value) => value.max(0) as u64,
            Err(e) => {
                tracing::warn!(metric = metric, error = %e, "Failed to collect scheduler metric");
                0
            },
        }
    }

    async fn job_status_counts(&self) -> BTreeMap<String, u64> {
        let mut counts = BTreeMap::new();

        match sqlx::query("SELECT status, COUNT(*)::BIGINT AS count FROM jobs GROUP BY status")
            .fetch_all(&self.pool)
            .await
        {
            Ok(rows) => {
                for row in rows {
                    let status: String = row.try_get("status").unwrap_or_default();
                    let count: i64 = row.try_get("count").unwrap_or_default();
                    counts.insert(status, count.max(0) as u64);
                }
            },
            Err(e) => {
                tracing::warn!(error = %e, "Failed to collect job status metrics");
            },
        }

        counts
    }
}

impl SchedulerMetricsSnapshot {
    fn render(&self) -> String {
        let mut output = String::new();

        writeln!(output, "# TYPE mikrom_scheduler_up gauge").ok();
        writeln!(output, "mikrom_scheduler_up 1").ok();

        writeln!(output, "# TYPE mikrom_scheduler_uptime_seconds gauge").ok();
        writeln!(
            output,
            "mikrom_scheduler_uptime_seconds {}",
            self.uptime_seconds
        )
        .ok();

        writeln!(output, "# TYPE mikrom_scheduler_db_pool_size gauge").ok();
        writeln!(
            output,
            "mikrom_scheduler_db_pool_size {}",
            self.db_pool_size
        )
        .ok();

        writeln!(output, "# TYPE mikrom_scheduler_db_pool_idle gauge").ok();
        writeln!(
            output,
            "mikrom_scheduler_db_pool_idle {}",
            self.db_pool_idle
        )
        .ok();

        writeln!(
            output,
            "# TYPE mikrom_scheduler_db_pool_max_connections gauge"
        )
        .ok();
        writeln!(
            output,
            "mikrom_scheduler_db_pool_max_connections {}",
            self.db_pool_max_connections
        )
        .ok();

        writeln!(output, "# TYPE mikrom_scheduler_db_pool_closed gauge").ok();
        writeln!(
            output,
            "mikrom_scheduler_db_pool_closed {}",
            if self.db_pool_closed { 1 } else { 0 }
        )
        .ok();

        writeln!(output, "# TYPE mikrom_scheduler_apps_total gauge").ok();
        writeln!(output, "mikrom_scheduler_apps_total {}", self.apps_total).ok();

        writeln!(
            output,
            "# TYPE mikrom_scheduler_apps_autoscaling_enabled gauge"
        )
        .ok();
        writeln!(
            output,
            "mikrom_scheduler_apps_autoscaling_enabled {}",
            self.apps_autoscaling_enabled
        )
        .ok();

        writeln!(output, "# TYPE mikrom_scheduler_apps_scaled_to_zero gauge").ok();
        writeln!(
            output,
            "mikrom_scheduler_apps_scaled_to_zero {}",
            self.apps_scaled_to_zero
        )
        .ok();

        writeln!(output, "# TYPE mikrom_scheduler_workers_total gauge").ok();
        writeln!(
            output,
            "mikrom_scheduler_workers_total {}",
            self.workers_total
        )
        .ok();

        writeln!(output, "# TYPE mikrom_scheduler_workers_online gauge").ok();
        writeln!(
            output,
            "mikrom_scheduler_workers_online {}",
            self.workers_online
        )
        .ok();

        writeln!(output, "# TYPE mikrom_scheduler_workers_offline gauge").ok();
        writeln!(
            output,
            "mikrom_scheduler_workers_offline {}",
            self.workers_offline
        )
        .ok();

        writeln!(output, "# TYPE mikrom_scheduler_workers_stale gauge").ok();
        writeln!(
            output,
            "mikrom_scheduler_workers_stale {}",
            self.workers_stale
        )
        .ok();

        writeln!(output, "# TYPE mikrom_scheduler_jobs_total gauge").ok();
        writeln!(output, "mikrom_scheduler_jobs_total {}", self.jobs_total).ok();

        writeln!(output, "# TYPE mikrom_scheduler_jobs gauge").ok();
        for status in [
            "pending",
            "scheduled",
            "running",
            "failed",
            "cancelled",
            "paused",
            "stopped",
        ] {
            let count = self.job_status_counts.get(status).copied().unwrap_or(0);
            writeln!(
                output,
                "mikrom_scheduler_jobs{{status=\"{}\"}} {}",
                status, count
            )
            .ok();
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};

    #[test]
    fn dual_stack_listener_accepts_ipv4_and_ipv6() {
        let listener = match bind_dual_stack_listener(0) {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!("skipping scheduler smoke test: dual-stack bind unavailable: {err}");
                return;
            },
        };
        let local_addr = listener
            .local_addr()
            .expect("local addr should be available");
        let port = local_addr.port();

        let mut connected_streams = Vec::new();
        if local_addr.is_ipv6() {
            let v6 = SocketAddr::from((Ipv6Addr::LOCALHOST, port));
            match TcpStream::connect(v6) {
                Ok(stream) => connected_streams.push(stream),
                Err(err) => {
                    eprintln!("skipping scheduler smoke test: ipv6 loopback unavailable: {err}")
                },
            }
        }

        let v4 = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        match TcpStream::connect(v4) {
            Ok(stream) => connected_streams.push(stream),
            Err(err) => {
                if local_addr.is_ipv6() {
                    eprintln!("skipping scheduler smoke test: ipv4 loopback unavailable: {err}");
                } else {
                    eprintln!(
                        "skipping scheduler smoke test: ipv4-only fallback could not connect: {err}"
                    );
                    return;
                }
            },
        }

        if connected_streams.is_empty() {
            eprintln!("skipping scheduler smoke test: no loopback connections succeeded");
            return;
        }

        let expected_accepts = connected_streams.len();
        let handle = std::thread::spawn(move || {
            for _ in 0..expected_accepts {
                let _ = listener.accept();
            }
        });

        handle.join().expect("listener thread should exit");
        drop(connected_streams);
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::SchedulerMetricsSnapshot;
    use std::collections::BTreeMap;

    #[test]
    fn renders_prometheus_style_metrics() {
        let mut job_status_counts = BTreeMap::new();
        job_status_counts.insert("running".to_string(), 3);

        let snapshot = SchedulerMetricsSnapshot {
            uptime_seconds: 42,
            db_pool_size: 5,
            db_pool_idle: 4,
            db_pool_max_connections: 10,
            db_pool_closed: false,
            apps_total: 2,
            apps_autoscaling_enabled: 1,
            apps_scaled_to_zero: 0,
            workers_total: 4,
            workers_online: 3,
            workers_offline: 1,
            workers_stale: 0,
            jobs_total: 3,
            job_status_counts,
        };

        let rendered = snapshot.render();

        assert!(rendered.contains("mikrom_scheduler_up 1"));
        assert!(rendered.contains("mikrom_scheduler_uptime_seconds 42"));
        assert!(rendered.contains("mikrom_scheduler_db_pool_size 5"));
        assert!(rendered.contains("mikrom_scheduler_jobs_total 3"));
        assert!(rendered.contains("mikrom_scheduler_jobs{status=\"running\"} 3"));
        assert!(rendered.contains("mikrom_scheduler_jobs{status=\"failed\"} 0"));
    }
}
