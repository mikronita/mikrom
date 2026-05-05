use chrono::Utc;
use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    Resource,
    trace::{BatchSpanProcessor, Sampler, SdkTracerProvider},
};
use opentelemetry_semantic_conventions::resource::{SERVICE_NAME, SERVICE_VERSION};
use serde::{Deserialize, Serialize};
use std::io::Write;
use tokio::sync::mpsc;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::{EnvFilter, Registry, layer::SubscriberExt, util::SubscriberInitExt};

const DEFAULT_LOG_LEVEL: &str = "info";
const DEFAULT_OTLP_ENDPOINT: &str = "http://localhost:4317";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub vm_id: String,
    pub app_id: String,
    pub source: String,
    pub message: String,
    pub timestamp: i64,
}

struct NatsWriter {
    tx: mpsc::Sender<Vec<u8>>,
}

impl Write for NatsWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = self.tx.try_send(buf.to_vec());
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct NatsMakeWriter {
    tx: mpsc::Sender<Vec<u8>>,
}

impl<'a> MakeWriter<'a> for NatsMakeWriter {
    type Writer = NatsWriter;

    fn make_writer(&self) -> Self::Writer {
        NatsWriter {
            tx: self.tx.clone(),
        }
    }
}

/// Initializes the tracing and logging system.
///
/// Configurable via environment variables:
/// - `LOG_FORMAT`: set to `json` for structured logging.
/// - `ENABLE_TELEMETRY`: set to `true` to enable OTLP tracing.
/// - `OTEL_EXPORTER_OTLP_ENDPOINT`: OTLP collector endpoint (default: http://localhost:4317).
/// - `NATS_URL`: if set, logs will also be sent to NATS.
pub fn init_telemetry(
    service_name: &str,
    service_version: &str,
    default_level: Option<&str>,
) -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_level.unwrap_or(DEFAULT_LOG_LEVEL)));

    let is_json = std::env::var("LOG_FORMAT").as_deref() == Ok("json");
    let enable_telemetry = std::env::var("ENABLE_TELEMETRY").as_deref() == Ok("true");
    let nats_url = std::env::var("NATS_URL").ok();

    let telemetry_layer = if enable_telemetry {
        let tracer = create_tracer(service_name, service_version)?;
        Some(tracing_opentelemetry::layer().with_tracer(tracer))
    } else {
        None
    };

    let (nats_layer, nats_handle) = if let Some(url) = nats_url {
        let (tx, rx) = mpsc::channel(1000);
        let service_name = service_name.to_string();
        let handle = tokio::spawn(async move {
            if let Err(e) = run_nats_logger(url, service_name, rx).await {
                eprintln!("NATS Logger failed: {}", e);
            }
        });

        let layer = tracing_subscriber::fmt::layer()
            .json()
            .with_writer(NatsMakeWriter { tx });
        (Some(layer), Some(handle))
    } else {
        (None, None)
    };

    // We don't strictly need to keep the handle, but it's good to know it's there
    let _ = nats_handle;

    let registry = Registry::default()
        .with(filter)
        .with(telemetry_layer)
        .with(nats_layer);

    if is_json {
        registry
            .with(tracing_subscriber::fmt::layer().json())
            .init();
    } else {
        registry.with(tracing_subscriber::fmt::layer()).init();
    }

    Ok(())
}

async fn run_nats_logger(
    url: String,
    service_name: String,
    mut rx: mpsc::Receiver<Vec<u8>>,
) -> anyhow::Result<()> {
    let nats = async_nats::connect(&url).await?;
    let subject = format!("mikrom.logs.{}.system", service_name);

    while let Some(data) = rx.recv().await {
        let message = String::from_utf8_lossy(&data).trim().to_string();
        if message.is_empty() {
            continue;
        }

        let entry = LogEntry {
            vm_id: "system".to_string(),
            app_id: service_name.clone(),
            source: "stdout".to_string(),
            message,
            timestamp: Utc::now().timestamp_nanos_opt().unwrap_or(0),
        };

        if let Ok(payload) = serde_json::to_vec(&vec![entry]) {
            let _ = nats.publish(subject.clone(), payload.into()).await;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_entry_serialization() {
        let entry = LogEntry {
            vm_id: "system".to_string(),
            app_id: "test-service".to_string(),
            source: "stdout".to_string(),
            message: "test message".to_string(),
            timestamp: 123456789,
        };

        let json = serde_json::to_string(&vec![entry]).unwrap();
        assert!(json.contains("\"vm_id\":\"system\""));
        assert!(json.contains("\"app_id\":\"test-service\""));
        assert!(json.contains("\"message\":\"test message\""));
    }

    #[tokio::test]
    async fn test_nats_writer_channel() {
        let (tx, mut rx) = mpsc::channel(10);
        let mut writer = NatsWriter { tx };

        writer.write_all(b"hello world").unwrap();
        let received = rx.recv().await.unwrap();
        assert_eq!(received, b"hello world");
    }
}
