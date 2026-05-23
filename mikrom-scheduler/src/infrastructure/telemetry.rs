use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DURATION_BUCKETS_SECONDS: [f64; 11] = [
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0,
];

#[derive(Clone, Default)]
pub struct SchedulerTelemetry {
    inner: Arc<Mutex<BTreeMap<TelemetryKey, TelemetryStats>>>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct TelemetryKey {
    component: &'static str,
    event: &'static str,
}

#[derive(Debug, Clone)]
struct TelemetryStats {
    calls: u64,
    errors: u64,
    duration_ns_sum: u128,
    duration_ns_max: u128,
    duration_bucket_counts: [u64; DURATION_BUCKETS_SECONDS.len()],
}

impl Default for TelemetryStats {
    fn default() -> Self {
        Self {
            calls: 0,
            errors: 0,
            duration_ns_sum: 0,
            duration_ns_max: 0,
            duration_bucket_counts: [0; DURATION_BUCKETS_SECONDS.len()],
        }
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
        let mut inner = self.inner.lock().expect("telemetry mutex poisoned");
        let stats = inner.entry(TelemetryKey { component, event }).or_default();
        stats.calls = stats.calls.saturating_add(1);
        if !success {
            stats.errors = stats.errors.saturating_add(1);
        }

        let nanos = duration.as_nanos();
        stats.duration_ns_sum = stats.duration_ns_sum.saturating_add(nanos);
        stats.duration_ns_max = stats.duration_ns_max.max(nanos);

        let duration_secs = duration.as_secs_f64();
        if let Some((index, _)) = DURATION_BUCKETS_SECONDS
            .iter()
            .enumerate()
            .find(|(_, bucket)| duration_secs <= **bucket)
        {
            stats.duration_bucket_counts[index] =
                stats.duration_bucket_counts[index].saturating_add(1);
        }
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

    pub fn render_metrics(&self) -> String {
        let snapshot = self.inner.lock().expect("telemetry mutex poisoned").clone();
        let mut output = String::new();

        writeln!(output, "# TYPE mikrom_scheduler_event_calls_total counter").ok();
        writeln!(output, "# TYPE mikrom_scheduler_event_errors_total counter").ok();
        writeln!(
            output,
            "# TYPE mikrom_scheduler_event_duration_seconds histogram"
        )
        .ok();
        writeln!(
            output,
            "# TYPE mikrom_scheduler_event_duration_seconds_max gauge"
        )
        .ok();

        for (key, stats) in snapshot {
            let labels = format!("component=\"{}\",event=\"{}\"", key.component, key.event);
            let duration_sum = stats.duration_ns_sum as f64 / 1_000_000_000.0;
            let duration_max = stats.duration_ns_max as f64 / 1_000_000_000.0;
            let mut cumulative = 0u64;

            writeln!(
                output,
                "mikrom_scheduler_event_calls_total{{{labels}}} {}",
                stats.calls
            )
            .ok();
            writeln!(
                output,
                "mikrom_scheduler_event_errors_total{{{labels}}} {}",
                stats.errors
            )
            .ok();

            for (bucket, count) in DURATION_BUCKETS_SECONDS
                .iter()
                .zip(stats.duration_bucket_counts.iter())
            {
                cumulative = cumulative.saturating_add(*count);
                writeln!(
                    output,
                    "mikrom_scheduler_event_duration_seconds_bucket{{{labels},le=\"{bucket}\"}} {}",
                    cumulative
                )
                .ok();
            }

            writeln!(
                output,
                "mikrom_scheduler_event_duration_seconds_bucket{{{labels},le=\"+Inf\"}} {}",
                stats.calls
            )
            .ok();
            writeln!(
                output,
                "mikrom_scheduler_event_duration_seconds_sum{{{labels}}} {:.6}",
                duration_sum
            )
            .ok();
            writeln!(
                output,
                "mikrom_scheduler_event_duration_seconds_count{{{labels}}} {}",
                stats.calls
            )
            .ok();
            writeln!(
                output,
                "mikrom_scheduler_event_duration_seconds_max{{{labels}}} {:.6}",
                duration_max
            )
            .ok();
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::{DURATION_BUCKETS_SECONDS, SchedulerTelemetry};
    use std::time::Duration;

    #[test]
    fn renders_histogram_metrics() {
        let telemetry = SchedulerTelemetry::default();
        telemetry.record("nats", "deploy", Duration::from_millis(7), true);
        telemetry.record("nats", "deploy", Duration::from_millis(12), false);

        let rendered = telemetry.render_metrics();

        assert!(rendered.contains("# TYPE mikrom_scheduler_event_duration_seconds histogram"));
        assert!(
            rendered.contains(
                "mikrom_scheduler_event_calls_total{component=\"nats\",event=\"deploy\"} 2"
            )
        );
        assert!(rendered.contains(
            "mikrom_scheduler_event_errors_total{component=\"nats\",event=\"deploy\"} 1"
        ));
        assert!(rendered.contains("mikrom_scheduler_event_duration_seconds_bucket{component=\"nats\",event=\"deploy\",le=\"0.01\"} 1"));
        assert!(rendered.contains("mikrom_scheduler_event_duration_seconds_bucket{component=\"nats\",event=\"deploy\",le=\"0.025\"} 2"));
        assert!(rendered.contains("mikrom_scheduler_event_duration_seconds_bucket{component=\"nats\",event=\"deploy\",le=\"+Inf\"} 2"));
        assert!(rendered.contains(
            "mikrom_scheduler_event_duration_seconds_count{component=\"nats\",event=\"deploy\"} 2"
        ));
        assert!(rendered.contains(
            "mikrom_scheduler_event_duration_seconds_max{component=\"nats\",event=\"deploy\"}"
        ));
        assert_eq!(DURATION_BUCKETS_SECONDS.len(), 11);
    }
}
