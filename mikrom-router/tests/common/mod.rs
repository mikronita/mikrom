use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::any,
};
use mikrom_router::health::RouterHealth;
use mikrom_router::proxy::{MikromProxy, RouterMetricsCounters};
use mikrom_router::state::{Route, State};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use pingora::lb::LoadBalancer;
use pingora::lb::selection::RoundRobin;
use pingora::prelude::*;
use std::fmt::Write;
use std::sync::Arc;
use std::sync::Once;
use tokio::sync::{RwLock, mpsc};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

static INIT: Once = Once::new();

pub(crate) fn init_test_tracing() {
    INIT.call_once(|| {
        use opentelemetry::trace::TracerProvider as _;
        use opentelemetry_sdk::trace::TracerProvider;

        opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

        let provider = TracerProvider::builder().build();
        let tracer = provider.tracer("mikrom-router-test");
        let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);

        let _ = tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer())
            .with(telemetry)
            .try_init();
    });
}

async fn dummy_upstream_handler(headers: HeaderMap) -> (StatusCode, String) {
    let mut echo = String::new();
    for (name, value) in &headers {
        let _ = writeln!(echo, "{name}: {}", value.to_str().unwrap_or(""));
    }
    (StatusCode::OK, echo)
}

pub(crate) struct TestEnv {
    pub(crate) proxy_url: String,
    pub(crate) state: Arc<RwLock<State>>,
}

#[allow(clippy::too_many_lines)]
pub(crate) async fn setup_test_env(rps_limit: isize, use_ipv6: bool) -> Option<TestEnv> {
    init_test_tracing();

    let app = Router::new().fallback(any(dummy_upstream_handler));
    let bind_addr = if use_ipv6 { "[::1]:0" } else { "127.0.0.1:0" };
    let listener = match tokio::net::TcpListener::bind(bind_addr).await {
        Ok(listener) => listener,
        Err(err) => {
            tracing::warn!(
                bind_addr = %bind_addr,
                error = %err,
                "Skipping router integration test environment because the sandbox does not allow binding"
            );
            return None;
        },
    };
    let upstream_addr = match listener.local_addr() {
        Ok(addr) => addr,
        Err(err) => {
            tracing::warn!(
                bind_addr = %bind_addr,
                error = %err,
                "Skipping router integration test environment because the upstream socket could not be inspected"
            );
            return None;
        },
    };

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let state = Arc::new(RwLock::new(State::default()));
    let metrics = Arc::new(RouterMetricsCounters::new());
    let health = Arc::new(RouterHealth::new());

    let (proxy_addr_str, proxy_port) = match std::net::TcpListener::bind(bind_addr) {
        Ok(listener) => {
            let addr = listener.local_addr().unwrap();
            (addr.to_string(), addr.port())
        },
        Err(err) => {
            tracing::warn!(
                bind_addr = %bind_addr,
                error = %err,
                "Skipping router integration test environment because the proxy listener could not be bound"
            );
            return None;
        },
    };
    let proxy_url = if use_ipv6 {
        format!("http://[::1]:{proxy_port}")
    } else {
        format!("http://127.0.0.1:{proxy_port}")
    };

    let targets = vec![upstream_addr.to_string()];
    let lb = LoadBalancer::<RoundRobin>::try_from_iter(targets.as_slice()).unwrap();
    let lb_arc = Arc::new(lb);
    {
        let mut s = state.write().await;
        let route = Route {
            host: "localhost".to_string(),
            targets: targets.clone(),
            lb: lb_arc,
            use_tls: false,
            tls_alternative_cn: None,
        };

        s.routes.insert("localhost".to_string(), route.clone());
        s.routes.insert("127.0.0.1".to_string(), route.clone());
        s.routes.insert("[::1]".to_string(), route.clone());
        s.routes
            .insert(format!("localhost:{proxy_port}"), route.clone());
        s.routes
            .insert(format!("127.0.0.1:{proxy_port}"), route.clone());
        s.routes.insert(format!("[::1]:{proxy_port}"), route);
        drop(s);
    }

    let traffic_publisher = Arc::new(mikrom_router::traffic::RouterTrafficPublisher::new(
        "router-test".into(),
        mpsc::channel(1).0,
    ));
    let proxy = MikromProxy::new(
        state.clone(),
        health,
        false,
        None,
        metrics,
        Some(traffic_publisher),
        rps_limit,
    );

    std::thread::spawn(move || {
        let mut my_server = Server::new(None).expect("Failed to create server");
        my_server.bootstrap();

        let mut proxy_service = http_proxy_service(&my_server.configuration, proxy);
        proxy_service.add_tcp(&proxy_addr_str);

        my_server.add_service(proxy_service);
        my_server.run_forever();
    });

    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    Some(TestEnv { proxy_url, state })
}
