use anyhow::Result;
use mikrom_proto::telemetry::{DynTelemetryLayer, TelemetryProviders, build_telemetry_stack};
use std::future::Future;
use std::sync::{Arc, Mutex, Once, OnceLock};
use std::time::Duration;
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Dispatch, Event, Metadata, error, info};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::Registry;
use tracing_subscriber::util::SubscriberInitExt;

static TRACING_INIT: Once = Once::new();
static TELEMETRY_INIT: Once = Once::new();
static TELEMETRY_LAYER: OnceLock<Arc<Mutex<Vec<DynTelemetryLayer>>>> = OnceLock::new();
static TELEMETRY_PROVIDERS: OnceLock<Arc<TelemetryProviders>> = OnceLock::new();

#[derive(Clone)]
struct DeferredTelemetry {
    layers: Arc<Mutex<Vec<DynTelemetryLayer>>>,
}

impl DeferredTelemetry {
    fn new(layers: Arc<Mutex<Vec<DynTelemetryLayer>>>) -> Self {
        Self { layers }
    }

    fn with_layers<R>(&self, f: impl FnOnce(&[DynTelemetryLayer]) -> R) -> Option<R> {
        let guard = self.layers.lock().ok()?;
        Some(f(&guard))
    }
}

impl Layer<Registry> for DeferredTelemetry {
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

    fn max_level_hint(&self) -> Option<tracing_subscriber::filter::LevelFilter> {
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

pub fn init_bootstrap_tracing_once() {
    TRACING_INIT.call_once(|| {
        let telemetry_layer = Arc::new(Mutex::new(Vec::new()));
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
    let Some(stack) =
        build_telemetry_stack("mikrom-router", env!("CARGO_PKG_VERSION"), Some(router_id))?
    else {
        return Ok(());
    };

    let providers = Arc::new(stack.providers());
    providers.install_globals();
    let _ = TELEMETRY_PROVIDERS.set(providers);

    let layer = TELEMETRY_LAYER
        .get()
        .ok_or_else(|| anyhow::anyhow!("Tracing subscriber was not initialized"))?;

    layer
        .lock()
        .map_err(|_| anyhow::anyhow!("failed to lock tracing telemetry layer"))?
        .extend(stack.into_layers());

    tracing::callsite::rebuild_interest_cache();

    Ok(())
}

pub fn shutdown_telemetry() {
    if let Some(providers) = TELEMETRY_PROVIDERS.get() {
        providers.shutdown();
    }
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
    use super::{connect_with_backoff, server_conf, server_threads};
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

    #[test]
    fn server_threads_never_returns_zero() {
        assert_eq!(server_threads(0), 1);
        assert_eq!(server_threads(4), 4);
    }

    #[test]
    fn server_conf_uses_the_expected_upgrade_socket() {
        let conf = server_conf(0);
        assert_eq!(conf.upgrade_sock, "/tmp/mikrom_router_upgrade.sock");
        assert_eq!(conf.threads, 1);
        assert_eq!(conf.grace_period_seconds, Some(30));
    }
}
