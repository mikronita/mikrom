#![allow(clippy::missing_const_for_fn)]

use crate::app::config::{NatsUrl, RouterId};
use crate::app::runtime;
use crate::application::proxy::RouterMetricsCounters;
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

pub struct TelemetryLoop {
    nats_url: NatsUrl,
    nats_use_tls: bool,
    nats_certs_dir: Option<String>,
    metrics_counters: Arc<RouterMetricsCounters>,
    state: Arc<RwLock<State>>,
    router_id: RouterId,
}

impl TelemetryLoop {
    pub fn new(
        nats_url: NatsUrl,
        nats_use_tls: bool,
        nats_certs_dir: Option<String>,
        metrics_counters: Arc<RouterMetricsCounters>,
        state: Arc<RwLock<State>>,
        router_id: RouterId,
    ) -> Self {
        Self {
            nats_url,
            nats_use_tls,
            nats_certs_dir,
            metrics_counters,
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
                    let requests_total = self.metrics_counters.requests_total.load(Ordering::Relaxed);
                    #[allow(clippy::cast_precision_loss)]
                    let latency_avg_ms = if requests_total > 0 {
                        self.metrics_counters.latency_sum_ms.load(Ordering::Relaxed) as f32 / requests_total as f32
                    } else {
                        0.0
                    };

                    info!(
                        router_id = %self.router_id,
                        routes = state.routes.len(),
                        certificates = state.certificates.len(),
                        acme_tokens = state.acme_tokens.len(),
                        requests_total,
                        responses_2xx = self.metrics_counters.responses_2xx.load(Ordering::Relaxed),
                        responses_3xx = self.metrics_counters.responses_3xx.load(Ordering::Relaxed),
                        responses_4xx = self.metrics_counters.responses_4xx.load(Ordering::Relaxed),
                        responses_5xx = self.metrics_counters.responses_5xx.load(Ordering::Relaxed),
                        acme_hits = self.metrics_counters.acme_hits.load(Ordering::Relaxed),
                        acme_misses = self.metrics_counters.acme_misses.load(Ordering::Relaxed),
                        redirects = self.metrics_counters.redirects.load(Ordering::Relaxed),
                        rate_limited = self.metrics_counters.rate_limited.load(Ordering::Relaxed),
                        route_wait_timeouts = self.metrics_counters.route_wait_timeouts.load(Ordering::Relaxed),
                        latency_avg_ms = f64::from(latency_avg_ms),
                        "Telemetry snapshot"
                    );

            let metrics = RouterMetrics {
                        router_id: self.router_id.as_str().to_string(),
                        requests_total,
                        responses_2xx: self.metrics_counters.responses_2xx.load(Ordering::Relaxed),
                        responses_3xx: self.metrics_counters.responses_3xx.load(Ordering::Relaxed),
                        responses_4xx: self.metrics_counters.responses_4xx.load(Ordering::Relaxed),
                        responses_5xx: self.metrics_counters.responses_5xx.load(Ordering::Relaxed),
                        latency_avg_ms: f64::from(latency_avg_ms),
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
