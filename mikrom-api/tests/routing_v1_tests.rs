use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mikrom_api::auth::jwt::create_token;
use mikrom_api::create_app;
use mikrom_api::domain::user::{MockUserRepository, UserRole};
use mikrom_api::nats::{NatsClient, TypedNatsClient};
use mikrom_api::{AppState, domain::MockTenantRepository};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

struct OfflineNats;

#[async_trait::async_trait]
impl NatsClient for OfflineNats {
    async fn request_raw(&self, subject: String, _payload: Vec<u8>) -> anyhow::Result<Vec<u8>> {
        if subject == "mikrom.scheduler.list_apps" {
            let response = mikrom_proto::scheduler::ListAppsResponse { apps: vec![] };
            let mut buf = Vec::new();
            prost::Message::encode(&response, &mut buf)?;
            Ok(buf)
        } else {
            Err(anyhow::anyhow!("offline"))
        }
    }

    async fn publish_raw(&self, _subject: String, _payload: Vec<u8>) -> anyhow::Result<()> {
        Ok(())
    }

    async fn subscribe_raw(&self, _subject: String) -> anyhow::Result<async_nats::Subscriber> {
        Err(anyhow::anyhow!("offline"))
    }
}

#[allow(clippy::field_reassign_with_default)]
fn build_state() -> AppState {
    let mut state = AppState::default();
    state.jwt_secret = "test-secret".to_string();
    state.ctx.jwt_secret = state.jwt_secret.clone();
    state.nats = TypedNatsClient::new_custom(Arc::new(OfflineNats));
    state.ctx.nats = state.nats.clone();

    let mut user_repo = MockUserRepository::new();
    user_repo.expect_find_by_email().returning(|_| Ok(None));
    state.user_repo = Arc::new(user_repo);
    state.ctx.user_repo = state.user_repo.clone();

    let tenant_repo = Arc::new(MockTenantRepository::new());
    state.tenant_repo = tenant_repo.clone();
    state.ctx.tenant_repo = tenant_repo;

    state
}

#[tokio::test]
async fn v1_health_route_exists() {
    let app = create_app(build_state());

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_ne!(response.status(), StatusCode::NOT_FOUND);

    let stream = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/health/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stream.status(), StatusCode::OK);
    assert_eq!(stream.headers()["content-type"], "text/event-stream");

    let legacy_stream = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/health/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(legacy_stream.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn v1_auth_login_route_exists() {
    let state = build_state();
    let token = create_token(
        &Uuid::new_v4().to_string(),
        "test@example.com",
        &UserRole::User,
        &state.jwt_secret,
    )
    .unwrap();

    let app = create_app(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/login")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::from(
                    r#"{"email":"test@example.com","password":"password"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_ne!(response.status(), StatusCode::NOT_FOUND);
}
