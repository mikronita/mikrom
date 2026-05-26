use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use opentelemetry_proto::tonic::collector::logs::v1::{
    ExportLogsServiceRequest, ExportLogsServiceResponse,
    logs_service_server::{LogsService, LogsServiceServer},
};
use opentelemetry_proto::tonic::collector::metrics::v1::{
    ExportMetricsServiceRequest, ExportMetricsServiceResponse,
    metrics_service_server::{MetricsService, MetricsServiceServer},
};
use opentelemetry_proto::tonic::collector::trace::v1::{
    ExportTraceServiceRequest, ExportTraceServiceResponse,
    trace_service_server::{TraceService, TraceServiceServer},
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::TcpListenerStream;

static ENV_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

struct EnvVarGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(key);
        // SAFETY: the tests in this module serialize all env access through ENV_LOCK.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, previous }
    }

    fn remove(key: &'static str) -> Self {
        let previous = std::env::var_os(key);
        // SAFETY: the tests in this module serialize all env access through ENV_LOCK.
        unsafe {
            std::env::remove_var(key);
        }
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        // SAFETY: the tests in this module serialize all env access through ENV_LOCK.
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

#[derive(Clone)]
struct MockCollector {
    trace_tx: Arc<Mutex<mpsc::Sender<ExportTraceServiceRequest>>>,
    log_tx: Arc<Mutex<mpsc::Sender<ExportLogsServiceRequest>>>,
    metric_tx: Arc<Mutex<mpsc::Sender<ExportMetricsServiceRequest>>>,
}

impl MockCollector {
    fn new(
        trace_tx: mpsc::Sender<ExportTraceServiceRequest>,
        log_tx: mpsc::Sender<ExportLogsServiceRequest>,
        metric_tx: mpsc::Sender<ExportMetricsServiceRequest>,
    ) -> Self {
        Self {
            trace_tx: Arc::new(Mutex::new(trace_tx)),
            log_tx: Arc::new(Mutex::new(log_tx)),
            metric_tx: Arc::new(Mutex::new(metric_tx)),
        }
    }
}

#[tonic14::async_trait]
impl TraceService for MockCollector {
    async fn export(
        &self,
        request: tonic14::Request<ExportTraceServiceRequest>,
    ) -> Result<tonic14::Response<ExportTraceServiceResponse>, tonic14::Status> {
        self.trace_tx
            .lock()
            .expect("trace collector mutex poisoned")
            .try_send(request.into_inner())
            .expect("trace channel full");
        Ok(tonic14::Response::new(ExportTraceServiceResponse {
            partial_success: None,
        }))
    }
}

#[tonic14::async_trait]
impl LogsService for MockCollector {
    async fn export(
        &self,
        request: tonic14::Request<ExportLogsServiceRequest>,
    ) -> Result<tonic14::Response<ExportLogsServiceResponse>, tonic14::Status> {
        self.log_tx
            .lock()
            .expect("log collector mutex poisoned")
            .try_send(request.into_inner())
            .expect("log channel full");
        Ok(tonic14::Response::new(ExportLogsServiceResponse {
            partial_success: None,
        }))
    }
}

#[tonic14::async_trait]
impl MetricsService for MockCollector {
    async fn export(
        &self,
        request: tonic14::Request<ExportMetricsServiceRequest>,
    ) -> Result<tonic14::Response<ExportMetricsServiceResponse>, tonic14::Status> {
        self.metric_tx
            .lock()
            .expect("metric collector mutex poisoned")
            .try_send(request.into_inner())
            .expect("metric channel full");
        Ok(tonic14::Response::new(ExportMetricsServiceResponse {
            partial_success: None,
        }))
    }
}

async fn spawn_collector() -> (
    SocketAddr,
    mpsc::Receiver<ExportTraceServiceRequest>,
    mpsc::Receiver<ExportLogsServiceRequest>,
    mpsc::Receiver<ExportMetricsServiceRequest>,
) {
    let listener = tokio::net::TcpListener::bind("[::1]:0")
        .await
        .expect("failed to bind OTLP mock collector");
    let addr = listener.local_addr().expect("missing listener address");
    let stream = TcpListenerStream::new(listener);

    let (trace_tx, trace_rx) = mpsc::channel(4);
    let (log_tx, log_rx) = mpsc::channel(4);
    let (metric_tx, metric_rx) = mpsc::channel(4);
    let collector = MockCollector::new(trace_tx, log_tx, metric_tx);

    tokio::spawn(async move {
        tonic14::transport::Server::builder()
            .add_service(TraceServiceServer::new(collector.clone()))
            .add_service(LogsServiceServer::new(collector.clone()))
            .add_service(MetricsServiceServer::new(collector))
            .serve_with_incoming(stream)
            .await
            .expect("mock OTLP collector failed");
    });

    (addr, trace_rx, log_rx, metric_rx)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn telemetry_exports_traces_logs_and_metrics() {
    let _env_lock = ENV_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;

    let (addr, mut trace_rx, mut log_rx, mut metric_rx) = spawn_collector().await;

    // Configure the shared telemetry helper to point at the local collector.
    let _enabled = EnvVarGuard::set("ENABLE_TELEMETRY", "true");
    let _endpoint = EnvVarGuard::set("OTEL_EXPORTER_OTLP_ENDPOINT", &format!("http://{addr}"));

    {
        let _telemetry = mikrom_proto::telemetry::init_telemetry(
            "mikrom-smoke",
            env!("CARGO_PKG_VERSION"),
            None,
        )
        .expect("telemetry initialization failed");

        let span = tracing::info_span!("telemetry_smoke_span");
        let _entered = span.enter();
        tracing::info!("telemetry smoke log");
        mikrom_proto::telemetry::record_service_startup("mikrom-smoke");
    }

    let trace_request = tokio::time::timeout(Duration::from_secs(5), trace_rx.recv())
        .await
        .expect("timed out waiting for trace export")
        .expect("missing trace export");
    let log_request = tokio::time::timeout(Duration::from_secs(5), log_rx.recv())
        .await
        .expect("timed out waiting for log export")
        .expect("missing log export");
    let metric_request = tokio::time::timeout(Duration::from_secs(5), metric_rx.recv())
        .await
        .expect("timed out waiting for metric export")
        .expect("missing metric export");

    assert!(!trace_request.resource_spans.is_empty());
    assert!(!trace_request.resource_spans[0].scope_spans.is_empty());
    assert!(
        !trace_request.resource_spans[0].scope_spans[0]
            .spans
            .is_empty()
    );

    assert!(!log_request.resource_logs.is_empty());
    assert!(!log_request.resource_logs[0].scope_logs.is_empty());
    assert!(
        !log_request.resource_logs[0].scope_logs[0]
            .log_records
            .is_empty()
    );

    assert!(!metric_request.resource_metrics.is_empty());
    assert!(!metric_request.resource_metrics[0].scope_metrics.is_empty());
    assert!(
        !metric_request.resource_metrics[0].scope_metrics[0]
            .metrics
            .is_empty()
    );
}

#[test]
fn telemetry_helpers_respect_environment() {
    let _env_lock = ENV_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .blocking_lock();

    let _endpoint = EnvVarGuard::remove("OTEL_EXPORTER_OTLP_ENDPOINT");
    let _enabled = EnvVarGuard::remove("ENABLE_TELEMETRY");
    assert_eq!(
        mikrom_proto::telemetry::telemetry_endpoint(),
        "http://192.168.122.128:4317"
    );
    assert!(!mikrom_proto::telemetry::telemetry_enabled());

    let _enabled = EnvVarGuard::set("ENABLE_TELEMETRY", "true");
    let _endpoint = EnvVarGuard::set("OTEL_EXPORTER_OTLP_ENDPOINT", "http://127.0.0.1:4317");
    assert_eq!(
        mikrom_proto::telemetry::telemetry_endpoint(),
        "http://127.0.0.1:4317"
    );
    assert!(mikrom_proto::telemetry::telemetry_enabled());
}

#[test]
fn telemetry_stack_is_disabled_when_flag_is_false() {
    let _env_lock = ENV_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .blocking_lock();

    let _enabled = EnvVarGuard::set("ENABLE_TELEMETRY", "false");
    let stack = mikrom_proto::telemetry::build_telemetry_stack("test-service", "1.0.0", None)
        .expect("telemetry stack should build");

    assert!(stack.is_none());
}

#[test]
fn telemetry_stack_exposes_trace_and_log_layers_when_enabled() {
    let _env_lock = ENV_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .blocking_lock();

    let _enabled = EnvVarGuard::set("ENABLE_TELEMETRY", "true");
    let _endpoint = EnvVarGuard::set("OTEL_EXPORTER_OTLP_ENDPOINT", "http://127.0.0.1:4317");

    let stack = mikrom_proto::telemetry::build_telemetry_stack("test-service", "1.0.0", None)
        .expect("telemetry stack should build")
        .expect("telemetry stack should be enabled");

    let providers = stack.providers();
    let layers = stack.into_layers();

    assert_eq!(layers.len(), 2);
    drop(providers);
}
