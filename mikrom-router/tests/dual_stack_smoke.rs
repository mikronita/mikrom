use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::any,
};
use mikrom_router::application::proxy::{MikromProxy, RouterMetricsCounters};
use mikrom_router::domain::health::RouterHealth;
use mikrom_router::domain::state::{Route, State};
use pingora::lb::LoadBalancer;
use pingora::lb::selection::RoundRobin;
use pingora::listeners::TcpSocketOptions;
use pingora::prelude::*;
use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::Arc;
use tokio::sync::RwLock;

fn dual_stack_tcp_socket_options() -> TcpSocketOptions {
    let mut options = TcpSocketOptions::default();
    options.ipv6_only = Some(false);
    options
}

async fn upstream_handler(_: HeaderMap) -> (StatusCode, &'static str) {
    (StatusCode::OK, "upstream-ok")
}

#[tokio::test]
async fn proxy_listener_accepts_ipv4_and_ipv6() {
    let upstream_app = Router::new().fallback(any(upstream_handler));
    let upstream_listener = match tokio::net::TcpListener::bind("[::1]:0").await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("skipping router dual-stack smoke test: IPv6 bind unavailable: {err}");
            return;
        },
    };
    let upstream_addr = upstream_listener.local_addr().expect("upstream addr");
    tokio::spawn(async move {
        let _ = axum::serve(upstream_listener, upstream_app).await;
    });

    let proxy_listener = match TcpListener::bind("[::]:0") {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("skipping router dual-stack smoke test: proxy bind unavailable: {err}");
            return;
        },
    };
    let proxy_port = proxy_listener.local_addr().expect("proxy addr").port();
    drop(proxy_listener);

    let targets = vec![upstream_addr.to_string()];
    let lb = LoadBalancer::<RoundRobin>::try_from_iter(targets.as_slice()).unwrap();
    let mut routes = HashMap::new();
    let route = Route {
        host: "localhost".to_string(),
        targets: targets.clone(),
        lb: Arc::new(lb),
        use_tls: false,
        tls_alternative_cn: None,
    };
    routes.insert("localhost".to_string(), route.clone());
    routes.insert("127.0.0.1".to_string(), route.clone());
    routes.insert("[::1]".to_string(), route.clone());
    routes.insert(format!("localhost:{proxy_port}"), route.clone());
    routes.insert(format!("127.0.0.1:{proxy_port}"), route.clone());
    routes.insert(format!("[::1]:{proxy_port}"), route.clone());

    let state = Arc::new(RwLock::new(State {
        routes,
        acme_tokens: HashMap::new(),
        certificates: HashMap::new(),
    }));
    let metrics = Arc::new(RouterMetricsCounters::new());
    let health = Arc::new(RouterHealth::new());
    let proxy = MikromProxy::new(state, health, false, None, metrics, None, 100);

    let proxy_addr_str = format!("[::]:{proxy_port}");
    std::thread::spawn(move || {
        let mut server = Server::new(None).expect("Failed to create server");
        server.bootstrap();

        let mut proxy_service = http_proxy_service(&server.configuration, proxy);
        proxy_service.add_tcp_with_settings(&proxy_addr_str, dual_stack_tcp_socket_options());

        server.add_service(proxy_service);
        server.run_forever();
    });

    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("reqwest client");

    let v4_url = format!("http://127.0.0.1:{proxy_port}/");
    let v6_url = format!("http://[::1]:{proxy_port}/");

    let v4_body = client
        .get(v4_url)
        .send()
        .await
        .expect("ipv4 request should succeed")
        .text()
        .await
        .expect("ipv4 body should decode");
    let v6_body = client
        .get(v6_url)
        .send()
        .await
        .expect("ipv6 request should succeed")
        .text()
        .await
        .expect("ipv6 body should decode");

    assert_eq!(v4_body, "upstream-ok");
    assert_eq!(v6_body, "upstream-ok");
}
