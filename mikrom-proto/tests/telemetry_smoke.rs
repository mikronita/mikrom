use async_trait::async_trait;
use opentelemetry_http::{Bytes, HttpClient, HttpError, Request, Response};
use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use prost14::Message;
use std::sync::{Arc, Mutex, OnceLock};
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Dispatch, Event, Metadata};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::Registry;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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

struct DeferredTelemetry {
    layers: Arc<Mutex<Vec<Box<dyn tracing_subscriber::Layer<Registry> + Send + Sync>>>>,
}

impl DeferredTelemetry {
    fn new(
        layers: Arc<Mutex<Vec<Box<dyn tracing_subscriber::Layer<Registry> + Send + Sync>>>>,
    ) -> Self {
        Self { layers }
    }

    fn with_layers<R>(
        &self,
        f: impl FnOnce(&[Box<dyn tracing_subscriber::Layer<Registry> + Send + Sync>]) -> R,
    ) -> Option<R> {
        let guard = self.layers.lock().ok()?;
        Some(f(&guard))
    }
}

impl tracing_subscriber::Layer<Registry> for DeferredTelemetry {
    fn on_register_dispatch(&self, dispatch: &Dispatch) {
        let _ = self.with_layers(|layers| {
            for layer in layers {
                layer.on_register_dispatch(dispatch);
            }
        });
    }

    fn on_layer(&mut self, _subscriber: &mut Registry) {}

    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        let mut saw_sometimes = false;
        let mut saw_always = false;
        let _ = self.with_layers(|layers| {
            for layer in layers {
                let interest = layer.register_callsite(metadata);
                if interest.is_always() {
                    saw_always = true;
                    break;
                }
                if interest.is_sometimes() {
                    saw_sometimes = true;
                }
            }
        });
        if saw_always {
            Interest::always()
        } else if saw_sometimes {
            Interest::sometimes()
        } else {
            Interest::never()
        }
    }

    fn enabled(&self, _metadata: &Metadata<'_>, _ctx: Context<'_, Registry>) -> bool {
        true
    }

    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, Registry>) {
        let _ = self.with_layers(|layers| {
            for layer in layers {
                layer.on_new_span(attrs, id, ctx.clone());
            }
        });
    }

    fn max_level_hint(&self) -> Option<LevelFilter> {
        self.with_layers(|layers| {
            layers
                .iter()
                .filter_map(tracing_subscriber::Layer::max_level_hint)
                .max()
        })
        .flatten()
    }

    fn on_record(&self, span: &Id, values: &Record<'_>, ctx: Context<'_, Registry>) {
        let _ = self.with_layers(|layers| {
            for layer in layers {
                layer.on_record(span, values, ctx.clone());
            }
        });
    }

    fn on_follows_from(&self, span: &Id, follows: &Id, ctx: Context<'_, Registry>) {
        let _ = self.with_layers(|layers| {
            for layer in layers {
                layer.on_follows_from(span, follows, ctx.clone());
            }
        });
    }

    fn event_enabled(&self, _event: &Event<'_>, _ctx: Context<'_, Registry>) -> bool {
        true
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, Registry>) {
        let _ = self.with_layers(|layers| {
            for layer in layers {
                layer.on_event(event, ctx.clone());
            }
        });
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, Registry>) {
        let _ = self.with_layers(|layers| {
            for layer in layers {
                layer.on_enter(id, ctx.clone());
            }
        });
    }

    fn on_exit(&self, id: &Id, ctx: Context<'_, Registry>) {
        let _ = self.with_layers(|layers| {
            for layer in layers {
                layer.on_exit(id, ctx.clone());
            }
        });
    }

    fn on_close(&self, id: Id, ctx: Context<'_, Registry>) {
        let _ = self.with_layers(|layers| {
            for layer in layers {
                layer.on_close(id.clone(), ctx.clone());
            }
        });
    }

    fn on_id_change(&self, old: &Id, new: &Id, ctx: Context<'_, Registry>) {
        let _ = self.with_layers(|layers| {
            for layer in layers {
                layer.on_id_change(old, new, ctx.clone());
            }
        });
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn telemetry_builds_without_panicking() {
    let _env_lock = ENV_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;

    let _enabled = EnvVarGuard::set("ENABLE_TELEMETRY", "true");
    let stack = mikrom_proto::telemetry::build_telemetry_stack_with_http_client(
        "mikrom-smoke",
        env!("CARGO_PKG_VERSION"),
        None,
        (),
    )
    .expect("telemetry stack should build")
    .expect("telemetry should be enabled");

    let providers = stack.providers();
    let layers = Arc::new(Mutex::new(stack.into_layers()));
    providers.install_globals();
    tracing_subscriber::registry()
        .with(DeferredTelemetry::new(layers))
        .init();

    {
        let span = tracing::info_span!("telemetry_smoke_span");
        let _entered = span.enter();
        tracing::info!("telemetry smoke log");
    }
    mikrom_proto::telemetry::record_service_startup("mikrom-smoke");

    providers.force_flush();

    std::mem::forget(providers);
}

#[test]
fn telemetry_helpers_respect_environment() {
    let _env_lock = ENV_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .blocking_lock();

    let _endpoint = EnvVarGuard::remove("DT_API_URL");
    let _otel_endpoint = EnvVarGuard::remove("OTEL_EXPORTER_OTLP_ENDPOINT");
    let _enabled = EnvVarGuard::remove("ENABLE_TELEMETRY");
    assert_eq!(
        mikrom_proto::telemetry::telemetry_endpoint(),
        "http://[::1]:4318/api/v2/otlp"
    );
    assert!(!mikrom_proto::telemetry::telemetry_enabled());

    let _enabled = EnvVarGuard::set("ENABLE_TELEMETRY", "true");
    let _endpoint = EnvVarGuard::set("DT_API_URL", "http://127.0.0.1:4318");
    assert_eq!(
        mikrom_proto::telemetry::telemetry_endpoint(),
        "http://127.0.0.1:4318/api/v2/otlp"
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
    let _endpoint = EnvVarGuard::set("DT_API_URL", "http://127.0.0.1:4318");
    let _token = EnvVarGuard::set("DT_API_TOKEN", "test-token");

    let stack = mikrom_proto::telemetry::build_telemetry_stack("test-service", "1.0.0", None)
        .expect("telemetry stack should build")
        .expect("telemetry stack should be enabled");

    let providers = stack.providers();
    let layers = stack.into_layers();

    assert_eq!(layers.len(), 2);
    drop(providers);
}
