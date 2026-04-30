use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mikrom_api::{AppState, create_app, repositories, scheduler};
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test]
async fn test_openapi_json_endpoint() {
    let mock_repo = repositories::user_repository::MockUserRepository::new();
    let db_url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://mikrom:mikrom_password@localhost:5432/mikrom_api_test".to_string()
    });
    let db_pool = sqlx::PgPool::connect_lazy(&db_url).unwrap();
    let app_repo = Arc::new(repositories::PostgresAppRepository::new(
        db_pool,
        "test-key".to_string(),
    ));

    let nats_url =
        std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
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
    let db_url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://mikrom:mikrom_password@localhost:5432/mikrom_api_test".to_string()
    });
    let db_pool = sqlx::PgPool::connect_lazy(&db_url).unwrap();
    let app_repo = Arc::new(repositories::PostgresAppRepository::new(
        db_pool,
        "test-key".to_string(),
    ));

    let nats_url =
        std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
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
