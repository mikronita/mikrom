use anyhow::Result;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::TracerProvider;
use std::future::Future;
use std::sync::{Arc, Mutex, Once, OnceLock};
use std::time::Duration;
use tracing::{error, info};
use tracing::Dispatch;
use tracing::subscriber::Interest;
use tracing::Metadata;
use tracing::span::{Attributes, Id, Record};
use tracing::Event;
use tracing_subscriber::layer::Context;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::Registry;
use tracing_subscriber::Layer;
use tracing_subscriber::util::SubscriberInitExt;

type DynTelemetryLayer = Box<dyn Layer<Registry> + Send + Sync + 'static>;

static TRACING_INIT: Once = Once::new();
static TELEMETRY_INIT: Once = Once::new();
static TELEMETRY_LAYER: OnceLock<Arc<Mutex<Option<DynTelemetryLayer>>>> = OnceLock::new();

#[derive(Clone)]
struct DeferredTelemetry {
    layer: Arc<Mutex<Option<DynTelemetryLayer>>>,
}

impl DeferredTelemetry {
    fn new(layer: Arc<Mutex<Option<DynTelemetryLayer>>>) -> Self {
        Self { layer }
    }

    fn with_layer<R>(&self, f: impl FnOnce(&DynTelemetryLayer) -> R) -> Option<R> {
        let guard = self.layer.lock().ok()?;
        guard.as_ref().map(f)
    }
}

impl Layer<Registry> for DeferredTelemetry {
    fn on_register_dispatch(&self, dispatch: &Dispatch) {
        let _ = self.with_layer(|layer| layer.on_register_dispatch(dispatch));
    }

    fn on_layer(&mut self, subscriber: &mut Registry) {
        if let Ok(mut guard) = self.layer.lock() && let Some(layer) = guard.as_mut() {
            layer.on_layer(subscriber);
        }
    }

    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        let _ = self.with_layer(|layer| layer.register_callsite(metadata));
        Interest::always()
    }

    fn enabled(&self, _metadata: &Metadata<'_>, _ctx: Context<'_, Registry>) -> bool {
        true
    }

    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, Registry>) {
        let _ = self.with_layer(|layer| layer.on_new_span(attrs, id, ctx));
    }

    fn max_level_hint(&self) -> Option<tracing_subscriber::filter::LevelFilter> {
        self.with_layer(|layer| layer.max_level_hint()).flatten()
    }

    fn on_record(&self, span: &Id, values: &Record<'_>, ctx: Context<'_, Registry>) {
        let _ = self.with_layer(|layer| layer.on_record(span, values, ctx));
    }

    fn on_follows_from(&self, span: &Id, follows: &Id, ctx: Context<'_, Registry>) {
        let _ = self.with_layer(|layer| layer.on_follows_from(span, follows, ctx));
    }

    fn event_enabled(&self, _event: &Event<'_>, _ctx: Context<'_, Registry>) -> bool {
        true
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, Registry>) {
        let _ = self.with_layer(|layer| layer.on_event(event, ctx));
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, Registry>) {
        let _ = self.with_layer(|layer| layer.on_enter(id, ctx));
    }

    fn on_exit(&self, id: &Id, ctx: Context<'_, Registry>) {
        let _ = self.with_layer(|layer| layer.on_exit(id, ctx));
    }

    fn on_close(&self, id: Id, ctx: Context<'_, Registry>) {
        let _ = self.with_layer(|layer| layer.on_close(id, ctx));
    }

    fn on_id_change(&self, old: &Id, new: &Id, ctx: Context<'_, Registry>) {
        let _ = self.with_layer(|layer| layer.on_id_change(old, new, ctx));
    }
}

pub fn init_bootstrap_tracing_once() {
    TRACING_INIT.call_once(|| {
        let telemetry_layer = Arc::new(Mutex::new(None));
        if TELEMETRY_LAYER.set(telemetry_layer.clone()).is_err() {
            eprintln!("Tracing telemetry layer was initialized more than once");
        }

        tracing_subscriber::registry()
            .with(DeferredTelemetry::new(telemetry_layer))
            .with(tracing_subscriber::EnvFilter::from_default_env())
            .with(tracing_subscriber::fmt::layer())
            .init();
    });
}

pub fn init_tracing_once(router_id: &str) {
    TELEMETRY_INIT.call_once(|| {
        init_bootstrap_tracing_once();

        if let Err(e) = enable_tracing(router_id) {
            eprintln!("Failed to initialize tracing: {e}");
        }
    });
}

fn enable_tracing(router_id: &str) -> Result<()> {
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
            format!("mikrom-router-{router_id}"),
        )]))
        .build();

    opentelemetry::global::set_tracer_provider(provider.clone());
    opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

    let tracer = provider.tracer("mikrom-router");
    let telemetry: DynTelemetryLayer = Box::new(tracing_opentelemetry::layer().with_tracer(tracer));

    let layer = TELEMETRY_LAYER
        .get()
        .ok_or_else(|| anyhow::anyhow!("Tracing subscriber was not initialized"))?;

    let mut guard = layer
        .lock()
        .map_err(|_| anyhow::anyhow!("failed to lock tracing telemetry layer"))?;
    *guard = Some(telemetry);

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

#[cfg(test)]
mod tests {
    use super::connect_with_backoff;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn connect_with_backoff_retries_until_success() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_clone = attempts.clone();

        let result = connect_with_backoff("test-component", Duration::from_millis(0), move || {
            let attempts = attempts_clone.clone();
            async move {
                let attempt = attempts.fetch_add(1, Ordering::SeqCst);
                if attempt < 2 {
                    Err(anyhow::anyhow!("temporary failure"))
                } else {
                    Ok("connected")
                }
            }
        })
        .await;

        assert_eq!(result, "connected");
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }
}
