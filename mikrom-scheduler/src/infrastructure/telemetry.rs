use std::future::Future;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use opentelemetry::KeyValue;
use opentelemetry::global;
use opentelemetry::metrics::{Counter, Histogram};

#[derive(Clone, Default)]
pub struct SchedulerTelemetry;

struct SchedulerOtelMetrics {
    calls: Counter<u64>,
    errors: Counter<u64>,
    duration: Histogram<f64>,
}

impl SchedulerOtelMetrics {
    fn get() -> &'static Self {
        static METRICS: OnceLock<SchedulerOtelMetrics> = OnceLock::new();
        METRICS.get_or_init(|| {
            let meter = global::meter("mikrom-scheduler");
            Self {
                calls: meter
                    .u64_counter("mikrom_scheduler_event_calls_total")
                    .build(),
                errors: meter
                    .u64_counter("mikrom_scheduler_event_errors_total")
                    .build(),
                duration: meter
                    .f64_histogram("mikrom_scheduler_event_duration_seconds")
                    .build(),
            }
        })
    }

    fn record(
        &self,
        component: &'static str,
        event: &'static str,
        duration: Duration,
        success: bool,
    ) {
        let attrs = [
            KeyValue::new("component", component),
            KeyValue::new("event", event),
        ];
        self.calls.add(1, &attrs);
        if !success {
            self.errors.add(1, &attrs);
        }
        self.duration.record(duration.as_secs_f64(), &attrs);
    }
}

impl SchedulerTelemetry {
    pub fn record(
        &self,
        component: &'static str,
        event: &'static str,
        duration: Duration,
        success: bool,
    ) {
        SchedulerOtelMetrics::get().record(component, event, duration, success);
    }

    pub async fn observe_result<T, E, F>(
        &self,
        component: &'static str,
        event: &'static str,
        future: F,
    ) -> Result<T, E>
    where
        F: Future<Output = Result<T, E>>,
    {
        let started = Instant::now();
        let result = future.await;
        self.record(component, event, started.elapsed(), result.is_ok());
        result
    }

    pub async fn observe_value<T, F>(
        &self,
        component: &'static str,
        event: &'static str,
        future: F,
    ) -> T
    where
        F: Future<Output = T>,
    {
        let started = Instant::now();
        let value = future.await;
        self.record(component, event, started.elapsed(), true);
        value
    }
}

#[cfg(test)]
mod tests {
    use super::SchedulerTelemetry;
    use std::time::Duration;

    #[test]
    fn records_otel_metrics_without_panicking() {
        let telemetry = SchedulerTelemetry;
        telemetry.record("nats", "deploy", Duration::from_millis(7), true);
        telemetry.record("nats", "deploy", Duration::from_millis(12), false);
    }

    #[tokio::test]
    async fn observes_result_and_value() {
        let telemetry = SchedulerTelemetry;

        let result = telemetry
            .observe_result("db", "fetch", async { Ok::<_, anyhow::Error>(42_u32) })
            .await
            .expect("result should succeed");
        assert_eq!(result, 42);

        let value = telemetry
            .observe_value("db", "count", async { 7_u32 })
            .await;
        assert_eq!(value, 7);
    }
}
