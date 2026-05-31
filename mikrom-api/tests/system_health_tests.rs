use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mikrom_api::AppState;
use mikrom_api::create_app;
use mikrom_api::domain::MockScheduler;
use mikrom_api::nats::{NatsClient, TypedNatsClient};
use std::sync::Arc;
use tokio::time::{Duration, timeout};
use tokio_stream::StreamExt;
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
    state.router_addr = "http://127.0.0.1:9".to_string();
    state
}

#[tokio::test]
async fn health_endpoint_reports_status() {
    let app = create_app(build_state());

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
    assert_eq!(json["services"]["API"], "ONLINE");
    assert_eq!(json["services"]["Scheduler"], "OFFLINE");
    assert_eq!(json["services"]["Builder"], "OFFLINE");
}

#[tokio::test]
#[ignore = "requires a stable SSE health snapshot fixture"]
async fn health_stream_endpoint_exists() {
    let app = create_app(build_state());

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
    assert_eq!(response.headers()["content-type"], "text/event-stream");

    let mut stream = response.into_body().into_data_stream();
    let chunk = timeout(Duration::from_millis(250), stream.next())
        .await
        .expect("health stream should emit an initial snapshot")
        .unwrap()
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&chunk).unwrap();
    assert_eq!(json["status"], "ok");
    assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(json["services"]["API"], "ONLINE");
    assert_eq!(json["services"]["Scheduler"], "OFFLINE");
    assert_eq!(json["services"]["Builder"], "OFFLINE");
    assert_eq!(json["services"]["Router"], "OFFLINE");
}
