use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    Resource,
    trace::{BatchSpanProcessor, Sampler, SdkTracerProvider},
};
use opentelemetry_semantic_conventions::resource::{SERVICE_NAME, SERVICE_VERSION};
use tracing_subscriber::{EnvFilter, Registry, layer::SubscriberExt, util::SubscriberInitExt};

pub fn init_telemetry(service_name: &str, service_version: &str) -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let registry = Registry::default().with(filter);

    let enable_telemetry = std::env::var("ENABLE_TELEMETRY").unwrap_or_default() == "true";

    if enable_telemetry {
        let otlp_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:4317".to_string());

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

        let tracer = provider.tracer("mikrom");
        let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);

        if std::env::var("LOG_FORMAT").unwrap_or_default() == "json" {
            registry
                .with(telemetry)
                .with(tracing_subscriber::fmt::layer().json())
                .init();
        } else {
            registry
                .with(telemetry)
                .with(tracing_subscriber::fmt::layer())
                .init();
        }
    } else {
        if std::env::var("LOG_FORMAT").unwrap_or_default() == "json" {
            registry
                .with(tracing_subscriber::fmt::layer().json())
                .init();
        } else {
            registry.with(tracing_subscriber::fmt::layer()).init();
        }
    }

    Ok(())
}
