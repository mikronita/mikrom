use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    Resource,
    trace::{BatchSpanProcessor, Sampler, SdkTracerProvider},
};
use opentelemetry_semantic_conventions::resource::{SERVICE_NAME, SERVICE_VERSION};
use tracing_subscriber::{EnvFilter, Registry, layer::SubscriberExt, util::SubscriberInitExt};

const DEFAULT_LOG_LEVEL: &str = "info";
const DEFAULT_OTLP_ENDPOINT: &str = "http://localhost:4317";

/// Initializes the tracing and logging system.
///
/// Configurable via environment variables:
/// - `LOG_FORMAT`: set to `json` for structured logging.
/// - `ENABLE_TELEMETRY`: set to `true` to enable OTLP tracing.
/// - `OTEL_EXPORTER_OTLP_ENDPOINT`: OTLP collector endpoint (default: http://localhost:4317).
pub fn init_telemetry(
    service_name: &str,
    service_version: &str,
    default_level: Option<&str>,
) -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_level.unwrap_or(DEFAULT_LOG_LEVEL)));

    let is_json = std::env::var("LOG_FORMAT").as_deref() == Ok("json");
    let enable_telemetry = std::env::var("ENABLE_TELEMETRY").as_deref() == Ok("true");

    let telemetry_layer = if enable_telemetry {
        let tracer = create_tracer(service_name, service_version)?;
        Some(tracing_opentelemetry::layer().with_tracer(tracer))
    } else {
        None
    };

    let registry = Registry::default().with(filter).with(telemetry_layer);

    if is_json {
        registry
            .with(tracing_subscriber::fmt::layer().json())
            .init();
    } else {
        registry.with(tracing_subscriber::fmt::layer()).init();
    }

    Ok(())
}

fn create_tracer(
    service_name: &str,
    service_version: &str,
) -> anyhow::Result<opentelemetry_sdk::trace::Tracer> {
    let otlp_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| DEFAULT_OTLP_ENDPOINT.to_string());

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(otlp_endpoint)
        .build()?;

    let resource = Resource::builder()
        .with_attributes(vec![
            KeyValue::new(SERVICE_NAME, service_name.to_string()),
            KeyValue::new(SERVICE_VERSION, service_version.to_string()),
        ])
        .build();

    let processor = BatchSpanProcessor::builder(exporter).build();

    let provider = SdkTracerProvider::builder()
        .with_span_processor(processor)
        .with_resource(resource)
        .with_sampler(Sampler::AlwaysOn)
        .build();

    Ok(provider.tracer("mikrom"))
}
