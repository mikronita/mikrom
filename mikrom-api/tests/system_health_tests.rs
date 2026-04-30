use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mikrom_api::{AppState, create_app, repositories, scheduler};
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test]
async fn test_health_endpoint_structure() {
    let mock_repo = repositories::user_repository::MockUserRepository::new();
    let db_url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://mikrom:mikrom_password@localhost:5432/mikrom_api_test".to_string()
    });
    let db_pool = sqlx::PgPool::connect_lazy(&db_url).unwrap();
    let app_repo = Arc::new(repositories::PostgresAppRepository::new(db_pool));

    let nats_url =
        std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();
    let state = AppState {
        user_repo: Arc::new(mock_repo),
        app_repo,
        scheduler: Arc::new(scheduler::MockScheduler::new()),
        nats_client,
        router_addr: "http://127.0.0.1:8080".to_string(),
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
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 2048)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["status"], "ok");
    assert!(json["services"].is_object());
    assert_eq!(json["services"]["API"], "ONLINE");
}

#[tokio::test]
async fn test_health_stream_endpoint() {
    let mock_repo = repositories::user_repository::MockUserRepository::new();
    let db_url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://mikrom:mikrom_password@localhost:5432/mikrom_api_test".to_string()
    });
    let db_pool = sqlx::PgPool::connect_lazy(&db_url).unwrap();
    let app_repo = Arc::new(repositories::PostgresAppRepository::new(db_pool));

    let nats_url =
        std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();
    let state = AppState {
        user_repo: Arc::new(mock_repo),
        app_repo,
        scheduler: Arc::new(scheduler::MockScheduler::new()),
        nats_client,
        router_addr: "http://127.0.0.1:8080".to_string(),
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
                .uri("/health/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
}
