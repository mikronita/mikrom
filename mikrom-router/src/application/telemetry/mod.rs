#![allow(clippy::missing_const_for_fn)]

use crate::app::config::{NatsUrl, RouterId};
use crate::app::runtime;
use crate::application::proxy::RouterMetricsCounters;
use crate::domain::health::{RouterHealth, RouterHealthState};
use crate::domain::state::State;
use async_nats::Client;
use async_trait::async_trait;
use mikrom_proto::router::RouterMetrics;
use pingora::server::ShutdownWatch;
use pingora::services::background::BackgroundService;
use prost::Message;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::sync::RwLock;
use tokio::time::{Duration, interval};
use tracing::{error, info};

const TELEMETRY_INTERVAL_SECS: u64 = 5;

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

async fn publish_best_effort(
    nats: &Client,
    subject: impl Into<String>,
    payload: Vec<u8>,
    context: &'static str,
) {
    let subject = subject.into();
    if let Err(e) = nats.publish(subject.clone(), payload.into()).await {
        error!(%context, %subject, error = %e, "Failed to publish telemetry to NATS");
    }
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

pub struct TelemetryLoop {
    nats_url: NatsUrl,
    nats_use_tls: bool,
    nats_certs_dir: Option<String>,
    metrics_counters: Arc<RouterMetricsCounters>,
    health: Arc<RouterHealth>,
    state: Arc<RwLock<State>>,
    router_id: RouterId,
}

impl TelemetryLoop {
    pub fn new(
        nats_url: NatsUrl,
        nats_use_tls: bool,
        nats_certs_dir: Option<String>,
        metrics_counters: Arc<RouterMetricsCounters>,
        health: Arc<RouterHealth>,
        state: Arc<RwLock<State>>,
        router_id: RouterId,
    ) -> Self {
        Self {
            nats_url,
            nats_use_tls,
            nats_certs_dir,
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
        let nats = runtime::connect_with_backoff(
            "Telemetry Loop NATS",
            std::time::Duration::from_secs(5),
            || async {
                crate::infrastructure::nats::connect_nats(
                    self.nats_url.as_str(),
                    self.nats_use_tls,
                    self.nats_certs_dir.as_deref(),
                )
                .await
            },
        )
        .await;
        info!("Telemetry Loop: Connected to NATS.");

        let mut interval = interval(Duration::from_secs(TELEMETRY_INTERVAL_SECS));
        info!(
            "Telemetry Loop: Starting loop for router: {}",
            self.router_id
        );

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let state = self.state.read().await;
                    let snapshot = build_snapshot(
                        &self.router_id,
                        &self.metrics_counters,
                        &self.health,
                        &state,
                    );
                    drop(state);

                    info!(
                        router_id = %snapshot.router_id,
                        routes = snapshot.routes,
                        certificates = snapshot.certificates,
                        acme_tokens = snapshot.acme_tokens,
                        requests_total = snapshot.requests_total,
                        responses_2xx = snapshot.responses_2xx,
                        responses_3xx = snapshot.responses_3xx,
                        responses_4xx = snapshot.responses_4xx,
                        responses_5xx = snapshot.responses_5xx,
                        acme_hits = snapshot.acme_hits,
                        acme_misses = snapshot.acme_misses,
                        redirects = snapshot.redirects,
                        rate_limited = snapshot.rate_limited,
                        route_wait_timeouts = snapshot.route_wait_timeouts,
                        latency_avg_ms = snapshot.latency_avg_ms,
                        health_state = ?snapshot.health_state,
                        health_live = snapshot.health_live,
                        health_ready = snapshot.health_ready,
                        health_dependencies_ready = snapshot.health_dependencies_ready,
                        health_control_plane_synced = snapshot.health_control_plane_synced,
                        health_wireguard_ready = snapshot.health_wireguard_ready,
                        health_upstream_ca_ready = snapshot.health_upstream_ca_ready,
                        health_has_startup_error = snapshot.health_has_startup_error,
                        "Telemetry snapshot"
                    );

                    let metrics = RouterMetrics {
                        router_id: snapshot.router_id.clone(),
                        requests_total: snapshot.requests_total,
                        responses_2xx: snapshot.responses_2xx,
                        responses_3xx: snapshot.responses_3xx,
                        responses_4xx: snapshot.responses_4xx,
                        responses_5xx: snapshot.responses_5xx,
                        latency_avg_ms: snapshot.latency_avg_ms,
                        timestamp: chrono::Utc::now().timestamp(),
                    };

                    let mut buf = Vec::new();
                    if let Err(e) = metrics.encode(&mut buf) {
                        error!("Telemetry Loop: Failed to encode telemetry: {e}");
                        continue;
                    }

                    publish_best_effort(
                        &nats,
                        crate::domain::subjects::router_metrics(self.router_id.as_str()),
                        buf,
                        "telemetry-loop",
                    )
                    .await;
                }
                _ = shutdown.changed() => {
                    info!("Telemetry Loop: Shutting down...");
                    break;
                }
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
}
