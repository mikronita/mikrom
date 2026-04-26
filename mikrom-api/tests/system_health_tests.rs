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
    let db_pool = sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap();
    let app_repo = Arc::new(repositories::PostgresAppRepository::new(db_pool));

    let state = AppState {
        user_repo: Arc::new(mock_repo),
        app_repo,
        scheduler: Arc::new(scheduler::MockScheduler::new()),
        scheduler_config: scheduler::SchedulerConfig::default(),
        builder_addr: "http://127.0.0.1:5004".to_string(),
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
    // Other services might be OFFLINE since they are not running
}

#[tokio::test]
async fn test_health_stream_endpoint() {
    let mock_repo = repositories::user_repository::MockUserRepository::new();
    let db_pool = sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap();
    let app_repo = Arc::new(repositories::PostgresAppRepository::new(db_pool));

    let state = AppState {
        user_repo: Arc::new(mock_repo),
        app_repo,
        scheduler: Arc::new(scheduler::MockScheduler::new()),
        scheduler_config: scheduler::SchedulerConfig::default(),
        builder_addr: "http://127.0.0.1:5004".to_string(),
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

    // Test that we can receive at least one event
    // Note: The stream has a 5s interval, so this might take a moment.
    // However, the first tick() usually happens immediately or we can reduce the interval for tests if needed.
    // Since we can't easily change the interval inside the handler without refactoring,
    // we'll just check that it's a valid SSE response.
}
