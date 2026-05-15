use crate::proxy::{MikromProxy, RouterMetricsCounters};
use crate::state::{Route, State};
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::any,
};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use pingora::lb::LoadBalancer;
use pingora::lb::selection::RoundRobin;
use pingora::prelude::*;
use std::fmt::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

static INIT: std::sync::Once = std::sync::Once::new();

fn init_test_tracing() {
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

struct TestEnv {
    proxy_url: String,
    state: Arc<RwLock<State>>,
    _upstream_addr: SocketAddr,
}

async fn setup_test_env(rps_limit: isize, use_ipv6: bool) -> Option<TestEnv> {
    init_test_tracing();
    // 1. Start Dummy Upstream (Using fallback to catch everything including /)
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

    // 2. Setup Proxy State
    let state = Arc::new(RwLock::new(State::default()));
    let metrics = Arc::new(RouterMetricsCounters::new());

    // 3. Find a free port for the proxy
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

    // 4. Configure routes to the upstream
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

        // Add all possible host variations that might come in the Host header
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

    let proxy = MikromProxy::new(state.clone(), false, None, metrics, rps_limit);

    std::thread::spawn(move || {
        let mut my_server = Server::new(None).expect("Failed to create server");
        my_server.bootstrap();

        let mut proxy_service = http_proxy_service(&my_server.configuration, proxy);
        proxy_service.add_tcp(&proxy_addr_str);

        my_server.add_service(proxy_service);
        my_server.run_forever();
    });

    // Wait for the server to bind and start listening
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    Some(TestEnv {
        proxy_url,
        state,
        _upstream_addr: upstream_addr,
    })
}

#[tokio::test]
async fn test_integration_acme_challenge() {
    let Some(env) = setup_test_env(100, false).await else {
        eprintln!("skipping router integration test: network bind unavailable");
        return;
    };
    {
        let mut s = env.state.write().await;
        s.acme_tokens
            .insert("test-token".to_string(), "auth-key-123".to_string());
    }

    let client = reqwest::Client::new();
    let res = client
        .get(format!(
            "{}/.well-known/acme-challenge/test-token",
            env.proxy_url
        ))
        .send()
        .await
        .expect("Failed to send request to proxy");

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.text().await.unwrap(), "auth-key-123");
}

#[tokio::test]
async fn test_integration_rate_limiting() {
    let Some(env) = setup_test_env(2, false).await else {
        eprintln!("skipping router integration test: network bind unavailable");
        return;
    }; // 2 RPS limit

    let client = reqwest::Client::new();

    // First 2 requests should pass
    for _ in 0..2 {
        let res = client
            .get(&env.proxy_url)
            .send()
            .await
            .expect("Failed to send request to proxy");
        assert_eq!(res.status(), StatusCode::OK);
    }

    // 3rd request should be rate limited
    let res = client
        .get(&env.proxy_url)
        .send()
        .await
        .expect("Failed to send request to proxy");
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(res.headers().contains_key("Retry-After"));
}

#[tokio::test]
async fn test_integration_security_headers() {
    let Some(env) = setup_test_env(100, false).await else {
        eprintln!("skipping router integration test: network bind unavailable");
        return;
    };

    let client = reqwest::Client::new();
    let res = client
        .get(&env.proxy_url)
        .send()
        .await
        .expect("Failed to send request to proxy");

    assert_eq!(res.status(), StatusCode::OK);
    let headers = res.headers();

    assert_eq!(
        headers.get("Strict-Transport-Security").unwrap(),
        "max-age=31536000; includeSubDomains; preload"
    );
    assert_eq!(headers.get("X-Content-Type-Options").unwrap(), "nosniff");
    assert_eq!(headers.get("X-Frame-Options").unwrap(), "SAMEORIGIN");
    assert_eq!(
        headers.get("Referrer-Policy").unwrap(),
        "strict-origin-when-cross-origin"
    );
}

#[tokio::test]
async fn test_integration_proxy_headers_and_tracing() {
    let Some(env) = setup_test_env(100, false).await else {
        eprintln!("skipping router integration test: network bind unavailable");
        return;
    };

    let client = reqwest::Client::new();
    let res = client
        .get(&env.proxy_url)
        .send()
        .await
        .expect("Failed to send request to proxy");

    assert_eq!(res.status(), StatusCode::OK);
    let body = res.text().await.unwrap();

    // Check if proxy headers were injected and received by upstream
    assert!(body.contains("x-forwarded-for: 127.0.0.1"));
    assert!(body.contains("x-real-ip: 127.0.0.1"));
    assert!(body.contains("x-forwarded-proto: http"));

    // Check if tracing context (traceparent) was propagated
    assert!(body.contains("traceparent:"));
}

#[tokio::test]
async fn test_integration_http_to_https_redirection() {
    let Some(env) = setup_test_env(100, false).await else {
        eprintln!("skipping router integration test: network bind unavailable");
        return;
    };

    // Add a certificate for "localhost" to trigger redirection
    {
        let mut s = env.state.write().await;
        s.certificates.insert(
            "localhost".to_string(),
            crate::state::Certificate {
                cert_pem: "fake-cert".to_string(),
                key_pem: "fake-key".to_string(),
                parsed_chain: Vec::new(),
                parsed_key: None,
            },
        );
    }

    let url = format!("{}/some/path", env.proxy_url);

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none()) // Don't follow so we can assert on 301
        .build()
        .unwrap();

    let res = client
        .get(&url)
        .header("Host", "localhost")
        .send()
        .await
        .expect("Failed to send request to proxy");

    assert_eq!(res.status(), StatusCode::MOVED_PERMANENTLY);
    assert_eq!(
        res.headers().get("Location").unwrap(),
        "https://localhost/some/path"
    );
}

#[tokio::test]
async fn test_integration_ipv6_connectivity() {
    let Some(env) = setup_test_env(100, true).await else {
        eprintln!("skipping router integration test: network bind unavailable");
        return;
    };

    let client = reqwest::Client::new();
    let res = client
        .get(&env.proxy_url)
        .send()
        .await
        .expect("Failed to send request to proxy via IPv6");

    assert_eq!(res.status(), StatusCode::OK);
    let body = res.text().await.unwrap();

    // Check if proxy headers were injected and received by upstream with IPv6 address
    assert!(body.contains("x-forwarded-for: ::1"));
    assert!(body.contains("x-real-ip: ::1"));
    assert!(body.contains("x-forwarded-proto: http"));
}
