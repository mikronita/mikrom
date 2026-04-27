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
    let db_pool = sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap();
    let app_repo = Arc::new(repositories::PostgresAppRepository::new(db_pool));

    let nats_client = async_nats::connect("nats://localhost:4222").await.unwrap();
    let state = AppState {
        user_repo: Arc::new(mock_repo),
        app_repo,
        scheduler: Arc::new(scheduler::MockScheduler::new()),
        nats_client,
        router_addr: "http://localhost:8080".to_string(),
        jwt_secret: "test".to_string(),
        master_key: "test".to_string(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        build_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
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
    let db_pool = sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap();
    let app_repo = Arc::new(repositories::PostgresAppRepository::new(db_pool));

    let nats_client = async_nats::connect("nats://localhost:4222").await.unwrap();
    let state = AppState {
        user_repo: Arc::new(mock_repo),
        app_repo,
        scheduler: Arc::new(scheduler::MockScheduler::new()),
        nats_client,
        router_addr: "http://localhost:8080".to_string(),
        jwt_secret: "test".to_string(),
        master_key: "test".to_string(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        build_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
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
