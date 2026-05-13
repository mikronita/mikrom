use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mikrom_api::test_utils::TestDb;
use mikrom_api::{AppState, create_app, repositories, scheduler};
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test]
async fn test_api_versioning_enforcement() {
    let db = TestDb::new().await;
    let db_pool = db.pool().clone();
    let app_repo = Arc::new(repositories::PostgresAppRepository::new(
        db_pool.clone(),
        "key".to_string(),
    ));

    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();

    let state = AppState {
        user_repo: Arc::new(repositories::user_repository::MockUserRepository::new()),
        app_repo,
        github_repo: Arc::new(mikrom_api::repositories::MockGithubRepository::default()),
        scheduler: Arc::new(scheduler::MockScheduler::new()),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        jwt_secret: "test".to_string(),
        master_key: "test".to_string(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        api_db: db_pool,
        acme_email: "admin@mikrom.spluca.org".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };
    let app = create_app(state);

    // 1. Verify /v1/health works
    let resp_v1 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp_v1.status(), StatusCode::OK);

    // 2. Verify legacy /health fails (404)
    let resp_legacy = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp_legacy.status(), StatusCode::NOT_FOUND);

    // 3. Verify /v1/auth/login exists (returns 405 or 400 instead of 404 because it's POST)
    let resp_auth_v1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/login")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_ne!(resp_auth_v1.status(), StatusCode::NOT_FOUND);

    // 4. Verify legacy /auth/login fails (404)
    let resp_auth_legacy = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/login")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp_auth_legacy.status(), StatusCode::NOT_FOUND);
}
