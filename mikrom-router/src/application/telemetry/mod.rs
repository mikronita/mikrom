#![allow(clippy::missing_const_for_fn)]

use crate::app::config::RouterId;
use crate::app::runtime;
use crate::application::proxy::RouterMetricsCounters;
use crate::domain::health::{RouterHealth, RouterHealthState};
use crate::domain::state::State;
use async_trait::async_trait;
use opentelemetry::KeyValue;
use opentelemetry::global;
use opentelemetry::metrics::{Counter, Gauge};
use pingora::server::ShutdownWatch;
use pingora::services::background::BackgroundService;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::Ordering;
use tokio::sync::RwLock;
use tracing::info;

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

#[derive(Default, Clone)]
struct RouterOtelSnapshot {
    requests_total: u64,
    responses_2xx: u64,
    responses_3xx: u64,
    responses_4xx: u64,
    responses_5xx: u64,
    acme_hits: u64,
    acme_misses: u64,
    redirects: u64,
    rate_limited: u64,
    route_wait_timeouts: u64,
}

struct RouterOtelMetrics {
    requests_total: Counter<u64>,
    responses_2xx: Counter<u64>,
    responses_3xx: Counter<u64>,
    responses_4xx: Counter<u64>,
    responses_5xx: Counter<u64>,
    acme_hits: Counter<u64>,
    acme_misses: Counter<u64>,
    redirects: Counter<u64>,
    rate_limited: Counter<u64>,
    route_wait_timeouts: Counter<u64>,
    routes: Gauge<u64>,
    certificates: Gauge<u64>,
    acme_tokens: Gauge<u64>,
    latency_avg_ms: Gauge<f64>,
    health_live: Gauge<u64>,
    health_ready: Gauge<u64>,
    health_dependencies_ready: Gauge<u64>,
    health_control_plane_synced: Gauge<u64>,
    health_wireguard_ready: Gauge<u64>,
    health_upstream_ca_ready: Gauge<u64>,
    health_has_startup_error: Gauge<u64>,
    last: std::sync::Mutex<RouterOtelSnapshot>,
}

impl RouterOtelMetrics {
    fn get() -> &'static Self {
        static METRICS: OnceLock<RouterOtelMetrics> = OnceLock::new();
        METRICS.get_or_init(|| {
            let meter = global::meter("mikrom-router");
            Self {
                requests_total: meter.u64_counter("mikrom_router_requests_total").build(),
                responses_2xx: meter
                    .u64_counter("mikrom_router_responses_2xx_total")
                    .build(),
                responses_3xx: meter
                    .u64_counter("mikrom_router_responses_3xx_total")
                    .build(),
                responses_4xx: meter
                    .u64_counter("mikrom_router_responses_4xx_total")
                    .build(),
                responses_5xx: meter
                    .u64_counter("mikrom_router_responses_5xx_total")
                    .build(),
                acme_hits: meter.u64_counter("mikrom_router_acme_hits_total").build(),
                acme_misses: meter.u64_counter("mikrom_router_acme_misses_total").build(),
                redirects: meter.u64_counter("mikrom_router_redirects_total").build(),
                rate_limited: meter
                    .u64_counter("mikrom_router_rate_limited_total")
                    .build(),
                route_wait_timeouts: meter
                    .u64_counter("mikrom_router_route_wait_timeouts_total")
                    .build(),
                routes: meter.u64_gauge("mikrom_router_routes_count").build(),
                certificates: meter.u64_gauge("mikrom_router_certificates_count").build(),
                acme_tokens: meter.u64_gauge("mikrom_router_acme_tokens_count").build(),
                latency_avg_ms: meter.f64_gauge("mikrom_router_latency_avg_ms").build(),
                health_live: meter.u64_gauge("mikrom_router_health_live").build(),
                health_ready: meter.u64_gauge("mikrom_router_health_ready").build(),
                health_dependencies_ready: meter
                    .u64_gauge("mikrom_router_health_dependencies_ready")
                    .build(),
                health_control_plane_synced: meter
                    .u64_gauge("mikrom_router_health_control_plane_synced")
                    .build(),
                health_wireguard_ready: meter
                    .u64_gauge("mikrom_router_health_wireguard_ready")
                    .build(),
                health_upstream_ca_ready: meter
                    .u64_gauge("mikrom_router_health_upstream_ca_ready")
                    .build(),
                health_has_startup_error: meter
                    .u64_gauge("mikrom_router_health_has_startup_error")
                    .build(),
                last: std::sync::Mutex::new(RouterOtelSnapshot::default()),
            }
        })
    }

    fn record_snapshot(&self, snapshot: &TelemetrySnapshot) {
        let attrs = [KeyValue::new("router_id", snapshot.router_id.clone())];
        {
            let mut last = self
                .last
                .lock()
                .expect("router otel metrics mutex poisoned");

            macro_rules! emit_delta {
                ($field:ident, $counter:expr) => {{
                    let current = snapshot.$field;
                    let previous = last.$field;
                    if current >= previous {
                        $counter.add(current - previous, &attrs);
                    }
                    last.$field = current;
                }};
            }

            emit_delta!(requests_total, self.requests_total);
            emit_delta!(responses_2xx, self.responses_2xx);
            emit_delta!(responses_3xx, self.responses_3xx);
            emit_delta!(responses_4xx, self.responses_4xx);
            emit_delta!(responses_5xx, self.responses_5xx);
            emit_delta!(acme_hits, self.acme_hits);
            emit_delta!(acme_misses, self.acme_misses);
            emit_delta!(redirects, self.redirects);
            emit_delta!(rate_limited, self.rate_limited);
            emit_delta!(route_wait_timeouts, self.route_wait_timeouts);
        }

        self.routes.record(snapshot.routes as u64, &attrs);
        self.certificates
            .record(snapshot.certificates as u64, &attrs);
        self.acme_tokens.record(snapshot.acme_tokens as u64, &attrs);
        self.latency_avg_ms.record(snapshot.latency_avg_ms, &attrs);
        self.health_live
            .record(u64::from(snapshot.health_live), &attrs);
        self.health_ready
            .record(u64::from(snapshot.health_ready), &attrs);
        self.health_dependencies_ready
            .record(u64::from(snapshot.health_dependencies_ready), &attrs);
        self.health_control_plane_synced
            .record(u64::from(snapshot.health_control_plane_synced), &attrs);
        self.health_wireguard_ready
            .record(u64::from(snapshot.health_wireguard_ready), &attrs);
        self.health_upstream_ca_ready
            .record(u64::from(snapshot.health_upstream_ca_ready), &attrs);
        self.health_has_startup_error
            .record(u64::from(snapshot.health_has_startup_error), &attrs);
    }
}

#[derive(Clone)]
pub struct TelemetryLoop {
    metrics_counters: Arc<RouterMetricsCounters>,
    health: Arc<RouterHealth>,
    state: Arc<RwLock<State>>,
    router_id: RouterId,
}

impl TelemetryLoop {
    pub fn new(
        metrics_counters: Arc<RouterMetricsCounters>,
        health: Arc<RouterHealth>,
        state: Arc<RwLock<State>>,
        router_id: RouterId,
    ) -> Self {
        Self {
            metrics_counters,
            health,
            state,
            router_id,
        }
    }
}

#[async_trait]
impl BackgroundService for TelemetryLoop {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        runtime::init_tracing_once(self.router_id.as_str());
        let initial_snapshot = {
            let app_state = self.state.read().await;
            build_snapshot(
                &self.router_id,
                &self.metrics_counters,
                &self.health,
                &app_state,
            )
        };
        RouterOtelMetrics::get().record_snapshot(&initial_snapshot);

        let loop_state = self.clone();
        let metrics_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                let snapshot = {
                    let app_state = loop_state.state.read().await;
                    build_snapshot(
                        &loop_state.router_id,
                        &loop_state.metrics_counters,
                        &loop_state.health,
                        &app_state,
                    )
                };
                RouterOtelMetrics::get().record_snapshot(&snapshot);
            }
        });

        info!("Telemetry Loop: starting OTel metrics recorder");

        let _ = shutdown.changed().await;
        info!("Telemetry Loop: shutdown signal received. Stopping...");
        metrics_task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::{RouterOtelMetrics, TelemetrySnapshot, build_snapshot};
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

    #[test]
    fn otel_metrics_record_snapshot_without_panicking() {
        let metrics = RouterMetricsCounters::new();
        metrics.requests_total.store(1, Ordering::Relaxed);
        metrics.responses_2xx.store(1, Ordering::Relaxed);

        let health = RouterHealth::new();
        health.mark_bootstrapped();

        let state = State::default();
        let snapshot = build_snapshot(
            &RouterId::from("router-otel-test"),
            &metrics,
            &health,
            &state,
        );

        RouterOtelMetrics::get().record_snapshot(&snapshot);
        metrics.requests_total.store(2, Ordering::Relaxed);
        metrics.responses_2xx.store(2, Ordering::Relaxed);
        let snapshot = build_snapshot(
            &RouterId::from("router-otel-test"),
            &metrics,
            &health,
            &state,
        );
        RouterOtelMetrics::get().record_snapshot(&snapshot);
    }
}
