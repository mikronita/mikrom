use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mikrom_api::test_utils::TestDb;
use mikrom_api::{AppState, create_app, repositories, scheduler};
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test]
async fn test_openapi_json_endpoint() {
    let mock_repo = repositories::user_repository::MockUserRepository::new();
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
        user_repo: Arc::new(mock_repo),
        app_repo,
        scheduler: Arc::new(scheduler::MockScheduler::new()),
        nats_client,
        router_addr: "http://localhost:8080".to_string(),
        jwt_secret: "test".to_string(),
        master_key: "test".to_string(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        api_db: db_pool,
        acme_email: "admin@mikrom.es".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
    };
    let app = create_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api-docs/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/json");
}

#[tokio::test]
async fn test_swagger_ui_endpoint() {
    let mock_repo = repositories::user_repository::MockUserRepository::new();
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
        user_repo: Arc::new(mock_repo),
        app_repo,
        scheduler: Arc::new(scheduler::MockScheduler::new()),
        nats_client,
        router_addr: "http://localhost:8080".to_string(),
        jwt_secret: "test".to_string(),
        master_key: "test".to_string(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        api_db: db_pool,
        acme_email: "admin@mikrom.es".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
    };
    let app = create_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/docs/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/html");
}
