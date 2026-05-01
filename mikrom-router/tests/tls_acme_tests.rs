use axum::Router;
use axum::routing::get;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mikrom_router::AppState;
use mikrom_router::acme::acme_challenge_handler;
use mikrom_router::tls::DatabaseCertResolver;
use moka::future::Cache;
use sqlx::PgPool;
use tower::ServiceExt;

async fn setup_test_db() -> Option<PgPool> {
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://mikrom:mikrom_password@localhost:5432/mikrom_router".to_string()
    });

    match PgPool::connect(&db_url).await {
        Ok(pool) => {
            // Clean up
            let _ = sqlx::query("DELETE FROM acme_challenges")
                .execute(&pool)
                .await;
            let _ = sqlx::query("DELETE FROM tls_certificates")
                .execute(&pool)
                .await;
            let _ = sqlx::query("DELETE FROM routes").execute(&pool).await;
            Some(pool)
        },
        Err(_) => None,
    }
}

#[tokio::test]
async fn test_acme_challenge_flow() {
    let db = match setup_test_db().await {
        Some(db) => db,
        None => return,
    };
    let state = AppState {
        db: db.clone(),
        cache: Cache::builder().build(),
        client: hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .build(hyper_util::client::legacy::connect::HttpConnector::new()),
    };

    // 1. Insert a dummy route to satisfy foreign key constraint
    let sni = "challenge.test.example.com";
    sqlx::query("INSERT INTO routes (hostname, target_url) VALUES ($1, $2)")
        .bind(sni)
        .bind("http://127.0.0.1:8080")
        .execute(&db)
        .await
        .unwrap();

    // 2. Insert a challenge
    let token = "test-token-123";
    let key_auth = "test-token-123.thumbprint";
    sqlx::query("INSERT INTO acme_challenges (token, key_auth, hostname) VALUES ($1, $2, $3)")
        .bind(token)
        .bind(key_auth)
        .bind(sni)
        .execute(&db)
        .await
        .unwrap();

    // 2. Test the handler directly
    let app = Router::new()
        .route(
            "/.well-known/acme-challenge/{token}",
            get(acme_challenge_handler),
        )
        .with_state(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/.well-known/acme-challenge/test-token-123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    assert_eq!(body, key_auth);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_database_cert_resolver() {
    let db = match setup_test_db().await {
        Some(db) => db,
        None => return,
    };
    let resolver = DatabaseCertResolver::new(db.clone());

    // 1. Initially, no cert found
    let sni = "resolver.test.example.com";
    assert!(resolver.load_cert_from_db(sni).is_none());

    // 2. Insert a dummy route to satisfy foreign key constraint
    sqlx::query("INSERT INTO routes (hostname, target_url) VALUES ($1, $2)")
        .bind(sni)
        .bind("http://127.0.0.1:8080")
        .execute(&db)
        .await
        .unwrap();

    // 3. Insert an EC certificate (smaller and easier to handle in tests)
    // Using the same EC cert/key from our previous manual test
    let cert_pem = "-----BEGIN CERTIFICATE-----
MIIBkDCCATegAwIBAgIUQelJbQVBu19wNuTkON0F0OJP4L0wCgYIKoZIzj0EAwIw
HjEcMBoGA1UEAwwTaG9uby5hcHBzLm1pa3JvbS5lczAeFw0yNjA1MDExMjE3NTla
Fw0yNzA1MDExMjE3NTlaMB4xHDAaBgNVBAMME2hvbm8uYXBwcy5taWtyb20uZXMw
WTATBgcqhkjOPQIBBggqhkjOPQMBBwNCAATgB/OfUgisiDK7xLl1MSm47FI/hG/I
OFmg+M7onP9CeqfPNW43VKC47U379S2vDRR/b2dkLpPsHmRNzmkyS11Go1MwUTAd
BgNVHQ4EFgQU6qM3/47ZBu2TFE37i8NJRwS/9RgwHwYDVR0jBBgwFoAU6qM3/47Z
Bu2TFE37i8NJRwS/9RgwDwYDVR0TAQH/BAUwAwEB/zAKBggqhkjOPQQDAgNHADBE
AiBFfPF8Qtdr2KiPjnC3kEYPhX/AhgV2wkswyeI7wq4xwwIgfJa2r9V/UnMTFmO2
3y4IbEiqKqhOf7qSXqMERFsAjoQ=
-----END CERTIFICATE-----";
    let key_pem = "-----BEGIN EC PRIVATE KEY-----
MHcCAQEEIBAawpDKHOLhR8VOwpoJKLp1o+t1hj+ymvNfLzu6Z25IoAoGCCqGSM49
AwEHoUQDQgAE4Afzn1IIrIgyu8S5dTEpuOxSP4RvyDhZoPjO6Jz/QnqnzzVuN1Sg
uO1N+/Utrw0Uf29nZC6T7B5kTc5pMktdRg==
-----END EC PRIVATE KEY-----";

    sqlx::query("INSERT INTO tls_certificates (hostname, cert_chain, private_key, expires_at) VALUES ($1, $2, $3, NOW() + INTERVAL '1 year')")
        .bind(sni)
        .bind(cert_pem)
        .bind(key_pem)
        .execute(&db)
        .await
        .unwrap();

    // 3. Now it should be found
    let cert_key = resolver
        .load_cert_from_db(sni)
        .expect("Should find cert in DB");
    assert!(!cert_key.cert.is_empty());
}
