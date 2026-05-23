use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mikrom_api::domain::MockUserRepository;
use mikrom_api::infrastructure::db::PostgresAppRepository;
use mikrom_api::nats::NatsClient;
use mikrom_api::test_utils::create_test_app_state;
use mikrom_api::{AppState, create_app};
use std::sync::Arc;
use tower::ServiceExt;

struct DummyNats;

#[async_trait::async_trait]
impl NatsClient for DummyNats {
    async fn request_raw(&self, _subject: String, _payload: Vec<u8>) -> anyhow::Result<Vec<u8>> {
        Err(anyhow::anyhow!("unexpected NATS request"))
    }

    async fn publish_raw(&self, _subject: String, _payload: Vec<u8>) -> anyhow::Result<()> {
        Ok(())
    }

    async fn subscribe_raw(&self, _subject: String) -> anyhow::Result<async_nats::Subscriber> {
        Err(anyhow::anyhow!("unexpected NATS subscribe"))
    }
}

async fn build_state() -> AppState {
    let db = sqlx::PgPool::connect_lazy("postgres://localhost/dummy").unwrap();
    let mut state = create_test_app_state(db.clone());
    let app_repo = Arc::new(PostgresAppRepository::new(db, "key".to_string()));
    state.app_repo = app_repo.clone();
    state.ctx.app_repo = app_repo;
    state.nats = mikrom_api::nats::TypedNatsClient::new_custom(Arc::new(DummyNats));
    state.ctx.nats = state.nats.clone();
    state.jwt_secret = "test".to_string();
    state.ctx.jwt_secret = "test".to_string();
    state.master_key = "test".to_string();
    state.ctx.master_key = "test".to_string();
    state
}

#[tokio::test]
async fn test_v1_health_routing() {
    let app = create_app(build_state().await);

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

    // 404 would mean prefix is missing or wrong
    // It might be 200 or 500 (if health check fails due to DB/NATS), but not 404
    assert_ne!(
        response.status(),
        StatusCode::NOT_FOUND,
        "GET /v1/health should not be 404"
    );
}

#[tokio::test]
async fn test_v1_auth_routing() {
    let state = build_state().await;
    let mut user_repo = MockUserRepository::new();
    user_repo.expect_find_by_email().returning(|_| Ok(None)); // Just return None to avoid 404/panic
    let mut state = state;
    state.user_repo = Arc::new(user_repo);
    state.ctx.user_repo = state.user_repo.clone();
    let app = create_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"email":"test@example.com","password":"password"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_ne!(
        response.status(),
        StatusCode::NOT_FOUND,
        "POST /v1/auth/login should not be 404"
    );
}

#[tokio::test]
async fn test_v1_apps_routing() {
    let app = create_app(build_state().await);

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/apps")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // It will likely be 401 Unauthorized because we don't provide a JWT, but not 404
    assert_ne!(
        response.status(),
        StatusCode::NOT_FOUND,
        "GET /v1/apps should not be 404"
    );
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
