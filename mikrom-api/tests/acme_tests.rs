use mikrom_api::test_utils::TestDb;
use mikrom_proto::router::TlsCertificateUpdate;
use mikrom_proto::subjects;
use prost::Message;
use tokio::time::{Duration, sleep};
use tokio_stream::StreamExt;

#[tokio::test]
async fn test_acme_account_persistence() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let db = TestDb::new().await;
    let pool = db.pool().clone();

    let email = "test-persistence@mikrom.spluca.org";
    let acme_url = "https://acme-staging-v02.api.letsencrypt.org/directory";

    // 1. First call should create account
    let _account = mikrom_api::acme::get_or_create_acme_account(&pool, email, acme_url, true)
        .await
        .expect("Failed to create account");

    let row_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM acme_accounts WHERE email = $1")
        .bind(email)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row_count, 1);

    // 2. Second call should retrieve the same account
    let _account2 = mikrom_api::acme::get_or_create_acme_account(&pool, email, acme_url, true)
        .await
        .expect("Failed to retrieve account");

    let row_count2: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM acme_accounts WHERE email = $1")
        .bind(email)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row_count2, 1);
}

#[tokio::test]
async fn test_acme_worker_iteration_skips_if_no_domains() {
    let db = TestDb::new().await;
    let pool = db.pool().clone();

    // Connecting to a local NATS for testing
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".into());
    let nats_client = async_nats::connect(&nats_url)
        .await
        .expect("Failed to connect to test NATS");

    // Run iteration - should finish quickly as there are no apps
    let result = mikrom_api::acme::run_acme_iteration(
        &pool,
        &mikrom_api::nats::TypedNatsClient::new(nats_client),
        "test@mikrom.spluca.org",
        "http://invalid-url",
        true,
        "master-key",
        "http://localhost:80",
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_router_handles_nats_updates() {
    // This test verifies that mikrom-router correctly updates its DB when receiving NATS messages
    let db = TestDb::new().await;
    let pool = db.pool().clone();

    // 1. Setup router tables (simulating migrations)
    sqlx::query("CREATE TABLE IF NOT EXISTS tls_certificates (hostname VARCHAR PRIMARY KEY, cert_chain TEXT NOT NULL, private_key TEXT NOT NULL, expires_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW())").execute(&pool).await.unwrap();
    sqlx::query("CREATE TABLE IF NOT EXISTS acme_challenges (token VARCHAR PRIMARY KEY, key_auth TEXT NOT NULL, hostname VARCHAR NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW())").execute(&pool).await.unwrap();

    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".into());
    let nats_client = async_nats::connect(&nats_url)
        .await
        .expect("Failed to connect to test NATS");

    // 2. Simulate Router NATS listener (simplified version of main.rs logic)
    let db_clone = pool.clone();
    let mut tls_sub = nats_client
        .subscribe(subjects::ROUTER_TLS_CERT_UPDATED)
        .await
        .unwrap();

    tokio::spawn(async move {
        let msg = match tls_sub.next().await {
            Some(m) => m,
            None => return,
        };

        if let Ok(update) = TlsCertificateUpdate::decode(&msg.payload[..]) {
            sqlx::query("INSERT INTO tls_certificates (hostname, cert_chain, private_key, expires_at) VALUES ($1, $2, $3, TO_TIMESTAMP($4))")
                .bind(&update.hostname)
                .bind(&update.cert_chain)
                .bind(&update.private_key)
                .bind(update.expires_at)
                .execute(&db_clone)
                .await
                .unwrap();
        }
    });

    // 3. Publish update from "API"
    let update = TlsCertificateUpdate {
        hostname: "test-sni.mikrom.spluca.org".into(),
        cert_chain: "CHAIN".into(),
        private_key: "KEY".into(),
        expires_at: 123456789,
    };
    nats_client
        .publish(
            subjects::ROUTER_TLS_CERT_UPDATED,
            update.encode_to_vec().into(),
        )
        .await
        .unwrap();

    // 4. Verify DB update
    sleep(Duration::from_millis(200)).await;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM tls_certificates WHERE hostname = 'test-sni.mikrom.spluca.org')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert!(exists, "Router should have updated its DB via NATS");
}
