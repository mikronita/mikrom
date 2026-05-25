#![allow(clippy::missing_const_for_fn)]

use crate::app::config::RouterId;
use crate::app::runtime;
use crate::application::proxy::RouterMetricsCounters;
use crate::domain::health::{RouterHealth, RouterHealthState};
use crate::domain::state::State;
use async_trait::async_trait;
use axum::{Router, routing::get};
use pingora::server::ShutdownWatch;
use pingora::services::background::BackgroundService;
use std::fmt::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::sync::RwLock;
use tracing::{error, info};

#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
struct TelemetrySnapshot {
    router_id: String,
    requests_total: u64,
    responses_2xx: u64,
    responses_3xx: u64,
    responses_4xx: u64,
    responses_5xx: u64,
    routes: usize,
    certificates: usize,
    acme_tokens: usize,
    acme_hits: u64,
    acme_misses: u64,
    redirects: u64,
    rate_limited: u64,
    route_wait_timeouts: u64,
    latency_avg_ms: f64,
    health_state: RouterHealthState,
    health_live: bool,
    health_ready: bool,
    health_dependencies_ready: bool,
    health_control_plane_synced: bool,
    health_wireguard_ready: bool,
    health_upstream_ca_ready: bool,
    health_has_startup_error: bool,
}

fn build_snapshot(
    router_id: &RouterId,
    metrics: &RouterMetricsCounters,
    health: &RouterHealth,
    state: &State,
) -> TelemetrySnapshot {
    let requests_total = metrics.requests_total.load(Ordering::Relaxed);
    #[allow(clippy::cast_precision_loss)]
    let latency_avg_ms = if requests_total > 0 {
        metrics.latency_sum_ms.load(Ordering::Relaxed) as f32 / requests_total as f32
    } else {
        0.0
    };

    let health_snapshot = health.snapshot();

    TelemetrySnapshot {
        router_id: router_id.as_str().to_string(),
        requests_total,
        responses_2xx: metrics.responses_2xx.load(Ordering::Relaxed),
        responses_3xx: metrics.responses_3xx.load(Ordering::Relaxed),
        responses_4xx: metrics.responses_4xx.load(Ordering::Relaxed),
        responses_5xx: metrics.responses_5xx.load(Ordering::Relaxed),
        routes: state.routes.len(),
        certificates: state.certificates.len(),
        acme_tokens: state.acme_tokens.len(),
        acme_hits: metrics.acme_hits.load(Ordering::Relaxed),
        acme_misses: metrics.acme_misses.load(Ordering::Relaxed),
        redirects: metrics.redirects.load(Ordering::Relaxed),
        rate_limited: metrics.rate_limited.load(Ordering::Relaxed),
        route_wait_timeouts: metrics.route_wait_timeouts.load(Ordering::Relaxed),
        latency_avg_ms: f64::from(latency_avg_ms),
        health_state: health_snapshot.state,
        health_live: health_snapshot.live,
        health_ready: health_snapshot.ready,
        health_dependencies_ready: health_snapshot.dependencies_ready,
        health_control_plane_synced: health_snapshot.control_plane_synced,
        health_wireguard_ready: health_snapshot.wireguard_ready,
        health_upstream_ca_ready: health_snapshot.upstream_ca_ready,
        health_has_startup_error: health_snapshot.startup_error.is_some(),
    }
}

#[derive(Clone)]
pub struct TelemetryLoop {
    metrics_port: u16,
    metrics_counters: Arc<RouterMetricsCounters>,
    health: Arc<RouterHealth>,
    state: Arc<RwLock<State>>,
    router_id: RouterId,
}

impl TelemetryLoop {
    pub fn new(
        metrics_port: u16,
        metrics_counters: Arc<RouterMetricsCounters>,
        health: Arc<RouterHealth>,
        state: Arc<RwLock<State>>,
        router_id: RouterId,
    ) -> Self {
        Self {
            metrics_port,
            metrics_counters,
            health,
            state,
            router_id,
        }
    }
}

async fn handle_metrics(
    axum::extract::State(state): axum::extract::State<Arc<TelemetryLoop>>,
) -> String {
    let app_state = state.state.read().await;
    let snapshot = build_snapshot(
        &state.router_id,
        &state.metrics_counters,
        &state.health,
        &app_state,
    );
    drop(app_state);
    format_snapshot(&snapshot)
}

#[allow(clippy::too_many_lines)]
fn format_snapshot(snapshot: &TelemetrySnapshot) -> String {
    let mut output = String::new();
    let router_id = &snapshot.router_id;

    // Helper to write a standard metric with only a router_id label.
    let write_metric =
        |out: &mut String, name: &str, type_: &str, value: &dyn std::fmt::Display| {
            let _ = writeln!(out, "# TYPE {name} {type_}");
            let _ = writeln!(out, "{name}{{router_id=\"{router_id}\"}} {value}");
        };

    write_metric(
        &mut output,
        "mikrom_router_requests_total",
        "counter",
        &snapshot.requests_total,
    );

    let _ = writeln!(output, "# TYPE mikrom_router_responses_total counter");
    let _ = writeln!(
        output,
        "mikrom_router_responses_total{{router_id=\"{router_id}\",family=\"2xx\"}} {}",
        snapshot.responses_2xx
    );
    let _ = writeln!(
        output,
        "mikrom_router_responses_total{{router_id=\"{router_id}\",family=\"3xx\"}} {}",
        snapshot.responses_3xx
    );
    let _ = writeln!(
        output,
        "mikrom_router_responses_total{{router_id=\"{router_id}\",family=\"4xx\"}} {}",
        snapshot.responses_4xx
    );
    let _ = writeln!(
        output,
        "mikrom_router_responses_total{{router_id=\"{router_id}\",family=\"5xx\"}} {}",
        snapshot.responses_5xx
    );

    write_metric(
        &mut output,
        "mikrom_router_routes_count",
        "gauge",
        &snapshot.routes,
    );
    write_metric(
        &mut output,
        "mikrom_router_certificates_count",
        "gauge",
        &snapshot.certificates,
    );
    write_metric(
        &mut output,
        "mikrom_router_acme_tokens_count",
        "gauge",
        &snapshot.acme_tokens,
    );
    write_metric(
        &mut output,
        "mikrom_router_acme_hits",
        "counter",
        &snapshot.acme_hits,
    );
    write_metric(
        &mut output,
        "mikrom_router_acme_misses",
        "counter",
        &snapshot.acme_misses,
    );
    write_metric(
        &mut output,
        "mikrom_router_redirects",
        "counter",
        &snapshot.redirects,
    );
    write_metric(
        &mut output,
        "mikrom_router_rate_limited",
        "counter",
        &snapshot.rate_limited,
    );
    write_metric(
        &mut output,
        "mikrom_router_route_wait_timeouts",
        "counter",
        &snapshot.route_wait_timeouts,
    );
    write_metric(
        &mut output,
        "mikrom_router_latency_avg_ms",
        "gauge",
        &snapshot.latency_avg_ms,
    );
    write_metric(
        &mut output,
        "mikrom_router_health_live",
        "gauge",
        &u32::from(snapshot.health_live),
    );
    write_metric(
        &mut output,
        "mikrom_router_health_ready",
        "gauge",
        &u32::from(snapshot.health_ready),
    );

    output
}

#[async_trait]
impl BackgroundService for TelemetryLoop {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        runtime::init_tracing_once(self.router_id.as_str());
        info!(
            "Telemetry Loop (Metrics HTTP Server): Starting on port {}...",
            self.metrics_port
        );

        let app_state = Arc::new(self.clone());
        let app = Router::new()
            .route("/metrics", get(handle_metrics))
            .with_state(app_state);

        let addr = SocketAddr::from(([0, 0, 0, 0], self.metrics_port));
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                error!(
                    "Telemetry Loop: Failed to bind HTTP server to {}: {}",
                    addr, e
                );
                return;
            },
        };

        let server_future = axum::serve(listener, app);

        tokio::select! {
            res = server_future => {
                if let Err(e) = res {
                    error!("Telemetry Loop HTTP server error: {}", e);
                }
            }
            _ = shutdown.changed() => {
                info!("Telemetry Loop (Metrics HTTP Server): Shutdown signal received. Stopping...");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TelemetrySnapshot, build_snapshot};
    use crate::app::config::RouterId;
    use crate::application::proxy::RouterMetricsCounters;
    use crate::domain::health::{RouterHealth, RouterHealthState};
    use crate::domain::state::State;
    use std::sync::atomic::Ordering;

    #[test]
    fn telemetry_snapshot_includes_health_state_and_counters() {
        let metrics = RouterMetricsCounters::new();
        metrics.requests_total.store(10, Ordering::Relaxed);
        metrics.latency_sum_ms.store(25, Ordering::Relaxed);
        metrics.responses_2xx.store(7, Ordering::Relaxed);
        metrics.responses_5xx.store(1, Ordering::Relaxed);
        metrics.acme_hits.store(2, Ordering::Relaxed);
        metrics.acme_misses.store(3, Ordering::Relaxed);
        metrics.redirects.store(4, Ordering::Relaxed);
        metrics.rate_limited.store(5, Ordering::Relaxed);
        metrics.route_wait_timeouts.store(6, Ordering::Relaxed);

        let health = RouterHealth::new();
        health.mark_bootstrapped();
        health.mark_dependencies_ready();
        health.mark_control_plane_synced();
        health.mark_wireguard_ready();
        health.mark_upstream_ca_ready();

        let snapshot: TelemetrySnapshot = build_snapshot(
            &RouterId::from("router-1"),
            &metrics,
            &health,
            &State::default(),
        );

        assert_eq!(snapshot.router_id, "router-1");
        assert_eq!(snapshot.requests_total, 10);
        assert_eq!(snapshot.responses_2xx, 7);
        assert_eq!(snapshot.responses_5xx, 1);
        assert_eq!(snapshot.acme_hits, 2);
        assert_eq!(snapshot.acme_misses, 3);
        assert_eq!(snapshot.redirects, 4);
        assert_eq!(snapshot.rate_limited, 5);
        assert_eq!(snapshot.route_wait_timeouts, 6);
        assert_eq!(snapshot.health_state, RouterHealthState::Ready);
        assert!(snapshot.health_live);
        assert!(snapshot.health_ready);
        assert!(!snapshot.health_has_startup_error);
    }

    #[tokio::test]
    async fn test_metrics_handler_output() {
        use super::{TelemetryLoop, handle_metrics};
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let metrics = Arc::new(RouterMetricsCounters::new());
        metrics.requests_total.store(42, Ordering::Relaxed);
        metrics.responses_2xx.store(40, Ordering::Relaxed);
        metrics.responses_5xx.store(2, Ordering::Relaxed);

        let health = Arc::new(RouterHealth::new());
        health.mark_bootstrapped();

        let state = Arc::new(RwLock::new(State::default()));
        let router_id = RouterId::from("test-router");

        let telemetry_loop = TelemetryLoop::new(9092, metrics, health, state, router_id);

        let response = handle_metrics(axum::extract::State(Arc::new(telemetry_loop))).await;

        assert!(response.contains("mikrom_router_requests_total{router_id=\"test-router\"} 42\n"));
        assert!(response.contains(
            "mikrom_router_responses_total{router_id=\"test-router\",family=\"2xx\"} 40\n"
        ));
        assert!(response.contains(
            "mikrom_router_responses_total{router_id=\"test-router\",family=\"5xx\"} 2\n"
        ));
        assert!(response.contains("mikrom_router_routes_count{router_id=\"test-router\"} 0\n"));
    }
}
