pub mod common;

use axum::http::StatusCode;
use common::setup_test_env;

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
    };

    let client = reqwest::Client::new();

    for _ in 0..2 {
        let res = client
            .get(&env.proxy_url)
            .send()
            .await
            .expect("Failed to send request to proxy");
        assert_eq!(res.status(), StatusCode::OK);
    }

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

    assert!(body.contains("x-forwarded-for: 127.0.0.1"));
    assert!(body.contains("x-real-ip: 127.0.0.1"));
    assert!(body.contains("x-forwarded-proto: http"));
    assert!(body.contains("traceparent:"));
}

#[tokio::test]
async fn test_integration_http_to_https_redirection() {
    let Some(env) = setup_test_env(100, false).await else {
        eprintln!("skipping router integration test: network bind unavailable");
        return;
    };

    {
        let mut s = env.state.write().await;
        s.certificates.insert(
            "localhost".to_string(),
            mikrom_router::state::Certificate {
                cert_pem: "fake-cert".to_string(),
                key_pem: "fake-key".to_string(),
                parsed_chain: Vec::new(),
                parsed_key: None,
            },
        );
    }

    let url = format!("{}/some/path", env.proxy_url);

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
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

    assert!(body.contains("x-forwarded-for: ::1"));
    assert!(body.contains("x-real-ip: ::1"));
    assert!(body.contains("x-forwarded-proto: http"));
}
