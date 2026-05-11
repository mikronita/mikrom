use crate::proxy::RouterMetricsCounters;
use async_nats::Client;
use mikrom_proto::router::RouterMetrics;
use prost::Message;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::time::{Duration, interval};
use tracing::{error, info};

pub async fn start_telemetry_loop(
    nats: Client,
    metrics_counters: Arc<RouterMetricsCounters>,
    router_id: String,
) {
    let mut interval = interval(Duration::from_secs(5));
    info!("Starting telemetry loop for router: {router_id}");

    loop {
        interval.tick().await;

        let metrics = RouterMetrics {
            router_id: router_id.clone(),
            requests_total: metrics_counters.requests_total.load(Ordering::Relaxed),
            responses_2xx: metrics_counters.responses_2xx.load(Ordering::Relaxed),
            responses_4xx: metrics_counters.responses_4xx.load(Ordering::Relaxed),
            responses_5xx: metrics_counters.responses_5xx.load(Ordering::Relaxed),
            latency_avg_ms: 0.0,
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
