use anyhow::Result;
use opentelemetry::KeyValue;
use opentelemetry::global;
use opentelemetry::metrics::Counter;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::metrics::PeriodicReader;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::{Resource, logs::SdkLoggerProvider, metrics::SdkMeterProvider};
use opentelemetry_semantic_conventions::resource::{SERVICE_NAME, SERVICE_VERSION};
use std::future::Future;
use std::time::Duration;
use tracing::{error, info};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::Registry;
use tracing_subscriber::util::SubscriberInitExt;

pub type DynTelemetryLayer = Box<dyn Layer<Registry> + Send + Sync + 'static>;

const DEFAULT_LOG_LEVEL: &str = "info";
const DEFAULT_OTLP_ENDPOINT: &str = "http://[::1]:4317";

#[derive(Clone)]
pub struct TelemetryProviders {
    tracer_provider: SdkTracerProvider,
    meter_provider: SdkMeterProvider,
    logger_provider: SdkLoggerProvider,
}

impl TelemetryProviders {
    fn new(
        tracer_provider: SdkTracerProvider,
        meter_provider: SdkMeterProvider,
        logger_provider: SdkLoggerProvider,
    ) -> Self {
        Self {
            tracer_provider,
            meter_provider,
            logger_provider,
        }
    }

    pub fn install_globals(&self) {
        global::set_tracer_provider(self.tracer_provider.clone());
        global::set_meter_provider(self.meter_provider.clone());
        global::set_text_map_propagator(
            opentelemetry_sdk::propagation::TraceContextPropagator::new(),
        );
    }

    pub fn shutdown(&self) {
        let _ = self.logger_provider.shutdown();
        let _ = self.meter_provider.shutdown();
        let _ = self.tracer_provider.shutdown();
    }
}

pub struct TelemetryStack {
    providers: TelemetryProviders,
    layers: Vec<DynTelemetryLayer>,
}

impl TelemetryStack {
    #[must_use]
    pub fn providers(&self) -> TelemetryProviders {
        self.providers.clone()
    }

    #[must_use]
    pub fn into_layers(self) -> Vec<DynTelemetryLayer> {
        self.layers
    }

    pub fn install_globals(&self) {
        self.providers.install_globals();
    }

    pub fn shutdown(&self) {
        self.providers.shutdown();
    }
}

pub struct TelemetryGuard(Option<TelemetryStack>);

impl TelemetryGuard {
    pub fn disabled() -> Self {
        Self(None)
    }
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Some(stack) = self.0.as_ref() {
            stack.shutdown();
        }
    }
}

#[must_use]
pub fn telemetry_endpoint() -> String {
    std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| DEFAULT_OTLP_ENDPOINT.to_string())
}

#[must_use]
pub fn telemetry_enabled() -> bool {
    std::env::var("ENABLE_TELEMETRY").as_deref() == Ok("true")
}

fn service_resource(
    service_name: &str,
    service_version: &str,
    instance_id: Option<&str>,
) -> Resource {
    let mut attributes = vec![
        KeyValue::new(SERVICE_NAME, service_name.to_string()),
        KeyValue::new(SERVICE_VERSION, service_version.to_string()),
    ];

    if let Some(instance_id) = instance_id {
        attributes.push(KeyValue::new(
            "service.instance.id",
            instance_id.to_string(),
        ));
    }

    Resource::builder().with_attributes(attributes).build()
}

fn build_providers(
    service_name: &str,
    service_version: &str,
    instance_id: Option<&str>,
) -> Result<TelemetryProviders> {
    let build = || {
        let endpoint = telemetry_endpoint();
        let resource = service_resource(service_name, service_version, instance_id);

        let span_exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint.clone())
            .build()?;
        let metric_exporter = opentelemetry_otlp::MetricExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint.clone())
            .build()?;
        let log_exporter = opentelemetry_otlp::LogExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build()?;

        let tracer_provider = SdkTracerProvider::builder()
            .with_batch_exporter(span_exporter)
            .with_resource(resource.clone())
            .build();

        let meter_provider = SdkMeterProvider::builder()
            .with_reader(PeriodicReader::builder(metric_exporter).build())
            .with_resource(resource.clone())
            .build();

        let logger_provider = SdkLoggerProvider::builder()
            .with_batch_exporter(log_exporter)
            .with_resource(resource)
            .build();

        Ok(TelemetryProviders::new(
            tracer_provider,
            meter_provider,
            logger_provider,
        ))
    };

    if tokio::runtime::Handle::try_current().is_ok() {
        build()
    } else {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let _guard = runtime.enter();
        build()
    }
}

pub fn build_telemetry_stack(
    service_name: &str,
    service_version: &str,
    instance_id: Option<&str>,
) -> Result<Option<TelemetryStack>> {
    if !telemetry_enabled() {
        return Ok(None);
    }

    let providers = build_providers(service_name, service_version, instance_id)?;

    let tracer = providers.tracer_provider.tracer(service_name.to_string());
    let trace_layer: DynTelemetryLayer =
        Box::new(tracing_opentelemetry::layer().with_tracer(tracer));
    let log_layer: DynTelemetryLayer =
        Box::new(OpenTelemetryTracingBridge::new(&providers.logger_provider));

    Ok(Some(TelemetryStack {
        providers,
        layers: vec![trace_layer, log_layer],
    }))
}

pub fn init_telemetry(
    service_name: &str,
    service_version: &str,
    default_level: Option<&str>,
) -> Result<TelemetryGuard> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(default_level.unwrap_or(DEFAULT_LOG_LEVEL))
    });
    let is_json = std::env::var("LOG_FORMAT").as_deref() == Ok("json");

    if !telemetry_enabled() {
        if is_json {
            Registry::default()
                .with(filter)
                .with(tracing_subscriber::fmt::layer().json())
                .init();
        } else {
            Registry::default()
                .with(filter)
                .with(tracing_subscriber::fmt::layer())
                .init();
        }

        return Ok(TelemetryGuard::disabled());
    }

    let providers = build_providers(service_name, service_version, None)?;
    providers.install_globals();

    let tracer = providers.tracer_provider.tracer(service_name.to_string());
    let trace_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    let log_layer = OpenTelemetryTracingBridge::new(&providers.logger_provider);

    if is_json {
        Registry::default()
            .with(filter)
            .with(trace_layer)
            .with(log_layer)
            .with(tracing_subscriber::fmt::layer().json())
            .init();
    } else {
        Registry::default()
            .with(filter)
            .with(trace_layer)
            .with(log_layer)
            .with(tracing_subscriber::fmt::layer())
            .init();
    }

    Ok(TelemetryGuard(Some(TelemetryStack {
        providers,
        layers: Vec::new(),
    })))
}

pub fn record_service_startup(service_name: &'static str) {
    let meter = global::meter(service_name);
    let counter: Counter<u64> = meter.u64_counter("mikrom_service_startups_total").build();
    counter.add(1, &[]);
}

pub async fn connect_with_backoff<T, F, Fut>(
    component: &'static str,
    retry_delay: Duration,
    mut connect: F,
) -> T
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut attempt = 0_u64;
    loop {
        attempt += 1;
        match connect().await {
            Ok(value) => {
                if attempt > 1 {
                    info!(component, attempts = attempt, "Connected after retries");
                }
                return value;
            },
            Err(err) => {
                error!(
                    component,
                    attempt,
                    error = %err,
                    "Failed to connect; retrying in {}s",
                    retry_delay.as_secs()
                );
                tokio::time::sleep(retry_delay).await;
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::connect_with_backoff;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn test_connect_with_backoff_succeeds() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_clone = attempts.clone();
        let result = connect_with_backoff("test", std::time::Duration::from_millis(1), move || {
            let attempts = attempts_clone.clone();
            async move {
                let current = attempts.fetch_add(1, Ordering::SeqCst);
                if current < 1 {
                    Err(anyhow::anyhow!("temporary"))
                } else {
                    Ok(42)
                }
            }
        })
        .await;

        assert_eq!(result, 42);
    }
}
