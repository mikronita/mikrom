use crate::proxy::RouterMetricsCounters;
use async_nats::Client;
use mikrom_proto::router::RouterMetrics;
use prost::Message;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::time::{Duration, interval};
use tracing::{error, info};

const TELEMETRY_INTERVAL_SECS: u64 = 5;

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
