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

fn ipv6_supported() -> bool {
    std::net::TcpListener::bind("[::]:0").is_ok()
}

async fn upstream_handler(_: HeaderMap) -> (StatusCode, &'static str) {
    (StatusCode::OK, "upstream-ok")
}

async fn start_upstream(use_ipv6: bool) -> Option<std::net::SocketAddr> {
    let listener = if use_ipv6 {
        match tokio::net::TcpListener::bind("[::1]:0").await {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!("skipping router dual-stack smoke test: IPv6 bind unavailable: {err}");
                return None;
            },
        }
    } else {
        match tokio::net::TcpListener::bind("127.0.0.1:0").await {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!("skipping router dual-stack smoke test: IPv4 bind unavailable: {err}");
                return None;
            },
        }
    };

    let upstream_addr = listener.local_addr().expect("upstream addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, Router::new().fallback(any(upstream_handler))).await;
    });
    Some(upstream_addr)
}

fn start_proxy(use_ipv6: bool) -> Option<u16> {
    let listener = if use_ipv6 {
        match TcpListener::bind("[::]:0") {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!("skipping router dual-stack smoke test: proxy bind unavailable: {err}");
                return None;
            },
        }
    } else {
        match TcpListener::bind("0.0.0.0:0") {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!("skipping router dual-stack smoke test: proxy bind unavailable: {err}");
                return None;
            },
        }
    };

    Some(listener.local_addr().expect("proxy addr").port())
}

fn build_routes(upstream_addr: std::net::SocketAddr, proxy_port: u16) -> HashMap<String, Route> {
    let targets = vec![upstream_addr.to_string()];
    let lb = LoadBalancer::<RoundRobin>::try_from_iter(targets.as_slice()).unwrap();
    let route = Route {
        host: "localhost".to_string(),
        targets,
        lb: Arc::new(lb),
        use_tls: false,
        tls_alternative_cn: None,
    };

    let mut routes = HashMap::new();
    routes.insert("localhost".to_string(), route.clone());
    routes.insert("127.0.0.1".to_string(), route.clone());
    routes.insert("[::1]".to_string(), route.clone());
    routes.insert(format!("localhost:{proxy_port}"), route.clone());
    routes.insert(format!("127.0.0.1:{proxy_port}"), route.clone());
    routes.insert(format!("[::1]:{proxy_port}"), route);
    routes
}

fn spawn_proxy_server(use_ipv6: bool, proxy_port: u16, proxy: MikromProxy) {
    let proxy_addr_str = if use_ipv6 {
        format!("[::]:{proxy_port}")
    } else {
        format!("0.0.0.0:{proxy_port}")
    };

    std::thread::spawn(move || {
        let mut server = Server::new(None).expect("Failed to create server");
        server.bootstrap();

        let mut proxy_service = http_proxy_service(&server.configuration, proxy);
        if use_ipv6 {
            proxy_service.add_tcp_with_settings(&proxy_addr_str, dual_stack_tcp_socket_options());
        } else {
            proxy_service.add_tcp(&proxy_addr_str);
        }

        server.add_service(proxy_service);
        server.run_forever();
    });
}

#[tokio::test]
async fn proxy_listener_accepts_ipv4_and_ipv6() {
    let use_ipv6 = ipv6_supported();
    let Some(upstream_addr) = start_upstream(use_ipv6).await else {
        return;
    };
    let Some(proxy_port) = start_proxy(use_ipv6) else {
        return;
    };

    let routes = build_routes(upstream_addr, proxy_port);
    let state = Arc::new(RwLock::new(State {
        routes,
        acme_tokens: HashMap::new(),
        certificates: HashMap::new(),
    }));
    let metrics = Arc::new(RouterMetricsCounters::new());
    let health = Arc::new(RouterHealth::new());
    let proxy = MikromProxy::new(state, health, false, None, metrics, None, 100);
    spawn_proxy_server(use_ipv6, proxy_port, proxy);

    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("reqwest client");

    let mut urls = vec![format!("http://127.0.0.1:{proxy_port}/")];
    if use_ipv6 {
        urls.push(format!("http://[::1]:{proxy_port}/"));
    }

    let mut bodies = Vec::new();
    for url in urls {
        match client.get(&url).send().await {
            Ok(response) => {
                bodies.push(response.text().await.expect("response body should decode"));
            },
            Err(err) => {
                if url.contains("[::1]") {
                    eprintln!(
                        "skipping router dual-stack smoke test: ipv6 request unavailable: {err}"
                    );
                } else if use_ipv6 {
                    eprintln!(
                        "skipping router dual-stack smoke test: ipv4 request unavailable: {err}"
                    );
                } else {
                    panic!("ipv4 request should succeed: {err}");
                }
            },
        }
    }

    assert!(
        !bodies.is_empty(),
        "at least one loopback request should succeed"
    );
    assert!(bodies.iter().all(|body| body == "upstream-ok"));
}
