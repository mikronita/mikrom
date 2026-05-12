#![allow(clippy::missing_const_for_fn)]

use crate::proxy::RouterMetricsCounters;
use async_nats::Client;
use async_trait::async_trait;
use mikrom_proto::router::RouterMetrics;
use pingora::server::ShutdownWatch;
use pingora::services::background::BackgroundService;
use prost::Message;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::time::{Duration, interval};
use tracing::{error, info};

const TELEMETRY_INTERVAL_SECS: u64 = 5;

pub struct TelemetryLoop {
    nats_url: String,
    metrics_counters: Arc<RouterMetricsCounters>,
    router_id: String,
}

impl TelemetryLoop {
    pub fn new(
        nats_url: String,
        metrics_counters: Arc<RouterMetricsCounters>,
        router_id: String,
    ) -> Self {
        Self {
            nats_url,
            metrics_counters,
            router_id,
        }
    }
}

#[async_trait]
impl BackgroundService for TelemetryLoop {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        crate::init_tracing_once(&self.router_id);
        // Connect to NATS
        let nats = loop {
            match async_nats::connect(&self.nats_url).await {
                Ok(client) => {
                    info!("Telemetry Loop: Connected to NATS.");
                    break client;
                },
                Err(e) => {
                    error!("Telemetry Loop: Failed to connect to NATS: {e}. Retrying in 5s...");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                },
            }
        };

        let mut interval = interval(Duration::from_secs(TELEMETRY_INTERVAL_SECS));
        info!(
            "Telemetry Loop: Starting loop for router: {}",
            self.router_id
        );

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let requests_total = self.metrics_counters.requests_total.load(Ordering::Relaxed);
                    #[allow(clippy::cast_precision_loss)]
                    let latency_avg_ms = if requests_total > 0 {
                        self.metrics_counters.latency_sum_ms.load(Ordering::Relaxed) as f32 / requests_total as f32
                    } else {
                        0.0
                    };

                    let metrics = RouterMetrics {
                        router_id: self.router_id.clone(),
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

                    let subject = format!("mikrom.metrics.router.{}", self.router_id);
                    if let Err(e) = nats.publish(subject, buf.into()).await {
                        error!("Telemetry Loop: Failed to publish telemetry to NATS: {e}");
                    }
                }
                _ = shutdown.changed() => {
                    info!("Telemetry Loop: Shutting down...");
                    break;
                }
            }
        }
    }
}

pub async fn start_telemetry_loop(
    nats: Client,
    metrics_counters: Arc<RouterMetricsCounters>,
    router_id: String,
) {
    let mut interval = interval(Duration::from_secs(TELEMETRY_INTERVAL_SECS));
    info!("Starting telemetry loop for router: {router_id}");

    loop {
        interval.tick().await;

        let requests_total = metrics_counters.requests_total.load(Ordering::Relaxed);
        #[allow(clippy::cast_precision_loss)]
        let latency_avg_ms = if requests_total > 0 {
            metrics_counters.latency_sum_ms.load(Ordering::Relaxed) as f32 / requests_total as f32
        } else {
            0.0
        };

        let metrics = RouterMetrics {
            router_id: router_id.clone(),
            requests_total,
            responses_2xx: metrics_counters.responses_2xx.load(Ordering::Relaxed),
            responses_3xx: metrics_counters.responses_3xx.load(Ordering::Relaxed),
            responses_4xx: metrics_counters.responses_4xx.load(Ordering::Relaxed),
            responses_5xx: metrics_counters.responses_5xx.load(Ordering::Relaxed),
            latency_avg_ms: f64::from(latency_avg_ms),
            timestamp: chrono::Utc::now().timestamp(),
        };

        let mut buf = Vec::new();
        if let Err(e) = metrics.encode(&mut buf) {
            error!("Failed to encode telemetry: {e}");
            continue;
        }

        let subject = format!("mikrom.metrics.router.{router_id}");
        if let Err(e) = nats.publish(subject, buf.into()).await {
            error!("Failed to publish telemetry to NATS: {e}");
        }
    }
}
