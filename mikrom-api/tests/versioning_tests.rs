mod common;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mikrom_api::create_app;
use mikrom_api::domain::MockUserRepository;
use mikrom_api::infrastructure::db::PostgresAppRepository;
use mikrom_api::test_utils::{TestDb, create_test_app_state};
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test]
async fn test_api_versioning_enforcement() {
    let db = TestDb::new().await;
    let db_pool = db.pool().clone();
    let app_repo = Arc::new(PostgresAppRepository::new(
        db_pool.clone(),
        "key".to_string(),
    ));

    let Some(nats_client) = common::get_nats_client_or_skip().await else {
        return;
    };

    let mut state = create_test_app_state(db_pool.clone());
    state.app_repo = app_repo.clone();
    state.user_repo = Arc::new(MockUserRepository::new());
    state.nats = mikrom_api::nats::TypedNatsClient::new(nats_client);
    state.jwt_secret = "test".to_string();
    state.master_key = "test".to_string();

    // Update ctx as well
    state.ctx.app_repo = app_repo;
    state.ctx.user_repo = state.user_repo.clone();
    state.ctx.nats = state.nats.clone();
    state.ctx.jwt_secret = state.jwt_secret.clone();
    state.ctx.master_key = state.master_key.clone();

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
