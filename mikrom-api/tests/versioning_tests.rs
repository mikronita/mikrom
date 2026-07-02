use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mikrom_api::AppState;
use mikrom_api::create_app;
use mikrom_api::domain::MockScheduler;
use mikrom_api::nats::{NatsClient, TypedNatsClient};
use std::sync::Arc;
use tower::ServiceExt;

struct OfflineNats;

#[async_trait::async_trait]
impl NatsClient for OfflineNats {
    async fn request_raw(&self, _subject: String, _payload: Vec<u8>) -> anyhow::Result<Vec<u8>> {
        Err(anyhow::anyhow!("offline"))
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
    state.nats = TypedNatsClient::new_custom(Arc::new(OfflineNats));
    state.ctx.nats = state.nats.clone();
    state.scheduler = Arc::new(MockScheduler::new());
    state.ctx.scheduler = state.scheduler.clone();
    state
}

#[tokio::test]
async fn api_routes_are_versioned_under_v1() {
    let app = create_app(build_state());

    let v1_health = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(v1_health.status(), StatusCode::OK);

    let legacy_health = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(legacy_health.status(), StatusCode::NOT_FOUND);

    let v1_health_stream = app
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
    assert_eq!(v1_health_stream.status(), StatusCode::OK);
    assert_eq!(
        v1_health_stream.headers()["content-type"],
        "text/event-stream"
    );

    let legacy_health_stream = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/health/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(legacy_health_stream.status(), StatusCode::NOT_FOUND);

    let v1_login = app
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
    assert_eq!(v1_login.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let legacy_login = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/login")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(legacy_login.status(), StatusCode::METHOD_NOT_ALLOWED);
}
