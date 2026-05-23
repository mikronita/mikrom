mod common;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mikrom_api::create_app;
use mikrom_api::test_utils::{TestDb, create_test_app_state};
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test]
async fn test_health_endpoint_structure() {
    let mock_user_repo = mikrom_api::domain::MockUserRepository::new();
    let db = TestDb::new().await;
    let db_pool = db.pool().clone();
    let app_repo = Arc::new(mikrom_api::infrastructure::db::PostgresAppRepository::new(
        db_pool.clone(),
        "key".to_string(),
    ));
    let Some(nats_client) = common::get_nats_client_or_skip().await else {
        return;
    };

    let mut state = create_test_app_state(db_pool.clone());
    state.user_repo = Arc::new(mock_user_repo);
    state.app_repo = app_repo.clone();
    state.ctx.user_repo = state.user_repo.clone();
    state.ctx.app_repo = state.app_repo.clone();
    state.nats = mikrom_api::nats::TypedNatsClient::new(nats_client);
    state.ctx.nats = state.nats.clone();
    state.api_db = db_pool;
    state.ctx.db = state.api_db.clone();

    let app = create_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/health")
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
    let mock_user_repo = mikrom_api::domain::MockUserRepository::new();
    let db = TestDb::new().await;
    let db_pool = db.pool().clone();
    let app_repo = Arc::new(mikrom_api::infrastructure::db::PostgresAppRepository::new(
        db_pool.clone(),
        "key".to_string(),
    ));
    let Some(nats_client) = common::get_nats_client_or_skip().await else {
        return;
    };

    let mut state = create_test_app_state(db_pool.clone());
    state.user_repo = Arc::new(mock_user_repo);
    state.app_repo = app_repo.clone();
    state.ctx.user_repo = state.user_repo.clone();
    state.ctx.app_repo = state.app_repo.clone();
    state.nats = mikrom_api::nats::TypedNatsClient::new(nats_client);
    state.ctx.nats = state.nats.clone();
    state.api_db = db_pool;
    state.ctx.db = state.api_db.clone();

    let app = create_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/health/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
