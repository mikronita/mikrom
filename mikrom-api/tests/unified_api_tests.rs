use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

use mikrom_api::domain::MockScheduler;
use mikrom_api::nats::{NatsClient, TypedNatsClient};
use mikrom_api::{AppState, create_app};
use std::sync::Arc;

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
async fn app() -> axum::Router {
    let mut state = AppState::default();
    state.nats = TypedNatsClient::new_custom(Arc::new(OfflineNats));
    state.ctx.nats = state.nats.clone();
    state.scheduler = Arc::new(MockScheduler::new());
    state.ctx.scheduler = state.scheduler.clone();
    create_app(state)
}

#[tokio::test]
async fn test_public_health_route_is_registered() {
    let router = app().await;
    let response = router
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
}

#[tokio::test]
async fn test_auth_and_app_routes_are_registered() {
    let router = app().await;

    let login_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/login")
                .header("content-type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login_response.status(), StatusCode::BAD_REQUEST);

    let apps_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/apps")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(apps_response.status(), StatusCode::UNAUTHORIZED);
}
