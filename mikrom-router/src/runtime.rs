use anyhow::Result;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::TracerProvider;
use std::future::Future;
use std::time::Duration;
use tracing::error;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

static TRACING_INIT: std::sync::Once = std::sync::Once::new();

pub fn init_tracing_once(router_id: &str) {
    TRACING_INIT.call_once(|| {
        if let Err(e) = init_tracing(&format!("mikrom-router-{router_id}")) {
            eprintln!("Failed to initialize tracing: {e}");
        }
    });
}

pub fn init_tracing(service_name: &str) -> Result<()> {
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(
            std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:4317".to_string()),
        )
        .build()?;

    let provider = TracerProvider::builder()
        .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
        .with_resource(Resource::new(vec![opentelemetry::KeyValue::new(
            "service.name",
            service_name.to_string(),
        )]))
        .build();

    opentelemetry::global::set_tracer_provider(provider.clone());
    opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

    let tracer = provider.tracer("mikrom-router");
    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .with(telemetry)
        .init();

    Ok(())
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
    loop {
        match connect().await {
            Ok(value) => return value,
            Err(err) => {
                error!(
                    component,
                    error = %err,
                    "Failed to connect; retrying in {}s",
                    retry_delay.as_secs()
                );
                tokio::time::sleep(retry_delay).await;
            },
        }
    }
}

#[must_use]
pub fn server_threads(requested: usize) -> usize {
    requested.max(1)
}

#[must_use]
pub fn server_conf(threads: usize) -> pingora::server::configuration::ServerConf {
    pingora::server::configuration::ServerConf {
        upgrade_sock: "/tmp/mikrom_router_upgrade.sock".to_string(),
        grace_period_seconds: Some(30),
        threads: server_threads(threads),
        ..Default::default()
    }
}
