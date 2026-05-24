use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use mikrom_api::AppState;
use mikrom_api::domain::MockScheduler;
use mikrom_api::domain::{
    MockAppRepository, MockGithubRepository, MockUserRepository, MockVolumeRepository,
};
use mikrom_api::nats::{NatsClient, TypedNatsClient};

#[path = "common/mod.rs"]
mod common;

struct DummyNats;

#[async_trait::async_trait]
impl NatsClient for DummyNats {
    async fn request_raw(&self, _subject: String, _payload: Vec<u8>) -> anyhow::Result<Vec<u8>> {
        Err(anyhow::anyhow!("unexpected NATS request"))
    }

    async fn publish_raw(&self, _subject: String, _payload: Vec<u8>) -> anyhow::Result<()> {
        Err(anyhow::anyhow!("unexpected NATS publish"))
    }

    async fn subscribe_raw(&self, _subject: String) -> anyhow::Result<async_nats::Subscriber> {
        Err(anyhow::anyhow!("unexpected NATS subscribe"))
    }
}

async fn build_state() -> AppState {
    AppState {
        ctx: mikrom_api::application::ApiContext::default(),
        user_repo: Arc::new(MockUserRepository::new()),
        app_repo: Arc::new(MockAppRepository::new()),
        volume_repo: Arc::new(MockVolumeRepository::new()),
        github_repo: Arc::new(MockGithubRepository::default()),
        scheduler: Arc::new(MockScheduler::new()),
        nats: TypedNatsClient::new_custom(Arc::new(DummyNats)),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        api_db: sqlx::PgPool::connect_lazy("postgres://localhost/dummy").unwrap(),
        jwt_secret: "test".to_string(),
        master_key: "test".to_string(),
        deployment_events: tokio::sync::broadcast::channel(100).0,
        acme_email: "test".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        mesh_status: tokio::sync::watch::channel(mikrom_api::domain::worker::MeshStatus::default())
            .0,
        active_deployment_flows: Arc::new(dashmap::DashSet::new()),
    }
}

#[tokio::test]
async fn test_openapi_json_endpoint() {
    let state = build_state().await;
    let app = mikrom_api::create_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/api-docs/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_swagger_ui_endpoint() {
    let state = build_state().await;
    let app = mikrom_api::create_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/docs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
