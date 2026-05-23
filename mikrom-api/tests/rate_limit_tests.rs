use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use axum::middleware;
use axum::routing::get;
use mikrom_api::auth::jwt::create_token;
use mikrom_api::domain::user::UserRole;
use mikrom_api::rate_limit::{RateLimitConfig, RateLimiter};
use std::net::SocketAddr;
use std::sync::Arc;
use tower::ServiceExt;

fn request_with_ip(method: &str, uri: &str, ip: SocketAddr) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(ConnectInfo(ip));
    request
}

fn request_with_ip_and_auth(method: &str, uri: &str, ip: SocketAddr, token: &str) -> Request<Body> {
    let mut request = request_with_ip(method, uri, ip);
    request
        .headers_mut()
        .insert("Authorization", format!("Bearer {token}").parse().unwrap());
    request
}

fn rate_limited_router(secret: &str, config: RateLimitConfig) -> Router {
    let limiter = Arc::new(RateLimiter::new(config, secret.to_string()).unwrap());

    Router::new()
        .route(
            "/v1/auth/login",
            axum::routing::post(|| async { StatusCode::OK }),
        )
        .route(
            "/v1/auth/register",
            axum::routing::post(|| async { StatusCode::CREATED }),
        )
        .route(
            "/v1/github/callback",
            get(|| async { StatusCode::SEE_OTHER }),
        )
        .route("/v1/github/install", get(|| async { StatusCode::OK }))
        .route_layer(middleware::from_fn_with_state(
            limiter,
            mikrom_api::rate_limit::rate_limit_middleware,
        ))
}

#[tokio::test]
async fn public_requests_are_limited_by_ip() {
    let router = rate_limited_router(
        "test-secret",
        RateLimitConfig {
            public_rpm: 1,
            rate_limit_auth_login_rpm: 1,
            rate_limit_auth_register_rpm: 1,
            rate_limit_github_install_rpm: 1,
            rate_limit_apps_create_rpm: 1,
            rate_limit_apps_deploy_rpm: 1,
            rate_limit_webhooks_github_generic_rpm: 1,
            rate_limit_webhooks_github_named_rpm: 1,
            authenticated_read_rpm: 1,
            authenticated_write_rpm: 1,
            authenticated_stream_rpm: 1,
            entry_ttl: std::time::Duration::from_secs(60),
            cleanup_interval: std::time::Duration::from_secs(1),
            trust_proxy_headers: false,
        },
    );
    let ip = SocketAddr::from(([127, 0, 0, 1], 30_001));

    let first = router
        .clone()
        .oneshot(request_with_ip("GET", "/v1/github/callback", ip))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::SEE_OTHER);

    let second = router
        .oneshot(request_with_ip("GET", "/v1/github/callback", ip))
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(second.headers()["retry-after"], "60");
}

#[tokio::test]
async fn authenticated_requests_are_limited_by_user() {
    let router = rate_limited_router(
        "test-secret",
        RateLimitConfig {
            public_rpm: 1,
            rate_limit_auth_login_rpm: 1,
            rate_limit_auth_register_rpm: 1,
            rate_limit_github_install_rpm: 1,
            rate_limit_apps_create_rpm: 1,
            rate_limit_apps_deploy_rpm: 1,
            rate_limit_webhooks_github_generic_rpm: 1,
            rate_limit_webhooks_github_named_rpm: 1,
            authenticated_read_rpm: 1,
            authenticated_write_rpm: 1,
            authenticated_stream_rpm: 1,
            entry_ttl: std::time::Duration::from_secs(60),
            cleanup_interval: std::time::Duration::from_secs(1),
            trust_proxy_headers: false,
        },
    );
    let ip = SocketAddr::from(([127, 0, 0, 1], 30_002));
    let token = create_token(
        "11111111-1111-1111-1111-111111111111",
        "rate-limit@example.com",
        &UserRole::User,
        "test-secret",
    )
    .unwrap();

    let first = router
        .clone()
        .oneshot(request_with_ip_and_auth(
            "GET",
            "/v1/github/install",
            ip,
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    let second = router
        .oneshot(request_with_ip_and_auth(
            "GET",
            "/v1/github/install",
            ip,
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(second.headers()["retry-after"], "60");
}

#[tokio::test]
async fn distinct_public_routes_do_not_share_buckets() {
    let router = rate_limited_router(
        "test-secret",
        RateLimitConfig {
            public_rpm: 1,
            rate_limit_auth_login_rpm: 1,
            rate_limit_auth_register_rpm: 1,
            rate_limit_github_install_rpm: 1,
            rate_limit_apps_create_rpm: 1,
            rate_limit_apps_deploy_rpm: 1,
            rate_limit_webhooks_github_generic_rpm: 1,
            rate_limit_webhooks_github_named_rpm: 1,
            authenticated_read_rpm: 1,
            authenticated_write_rpm: 1,
            authenticated_stream_rpm: 1,
            entry_ttl: std::time::Duration::from_secs(60),
            cleanup_interval: std::time::Duration::from_secs(1),
            trust_proxy_headers: false,
        },
    );
    let ip = SocketAddr::from(([127, 0, 0, 1], 30_003));

    let first = router
        .clone()
        .oneshot(request_with_ip("POST", "/v1/auth/login", ip))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    let second = router
        .clone()
        .oneshot(request_with_ip("POST", "/v1/auth/login", ip))
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);

    let register = router
        .oneshot(request_with_ip("POST", "/v1/auth/register", ip))
        .await
        .unwrap();
    assert_eq!(register.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn route_specific_limits_override_defaults() {
    let router = rate_limited_router(
        "test-secret",
        RateLimitConfig {
            public_rpm: 10,
            rate_limit_auth_login_rpm: 1,
            rate_limit_auth_register_rpm: 2,
            rate_limit_github_install_rpm: 10,
            rate_limit_apps_create_rpm: 10,
            rate_limit_apps_deploy_rpm: 10,
            rate_limit_webhooks_github_generic_rpm: 10,
            rate_limit_webhooks_github_named_rpm: 10,
            authenticated_read_rpm: 10,
            authenticated_write_rpm: 10,
            authenticated_stream_rpm: 10,
            entry_ttl: std::time::Duration::from_secs(60),
            cleanup_interval: std::time::Duration::from_secs(1),
            trust_proxy_headers: false,
        },
    );
    let ip = SocketAddr::from(([127, 0, 0, 1], 30_004));

    let login_1 = router
        .clone()
        .oneshot(request_with_ip("POST", "/v1/auth/login", ip))
        .await
        .unwrap();
    let login_2 = router
        .clone()
        .oneshot(request_with_ip("POST", "/v1/auth/login", ip))
        .await
        .unwrap();

    let register_1 = router
        .clone()
        .oneshot(request_with_ip("POST", "/v1/auth/register", ip))
        .await
        .unwrap();
    let register_2 = router
        .clone()
        .oneshot(request_with_ip("POST", "/v1/auth/register", ip))
        .await
        .unwrap();
    let register_3 = router
        .oneshot(request_with_ip("POST", "/v1/auth/register", ip))
        .await
        .unwrap();

    assert_eq!(login_1.status(), StatusCode::OK);
    assert_eq!(login_2.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(register_1.status(), StatusCode::CREATED);
    assert_eq!(register_2.status(), StatusCode::CREATED);
    assert_eq!(register_3.status(), StatusCode::TOO_MANY_REQUESTS);
}
