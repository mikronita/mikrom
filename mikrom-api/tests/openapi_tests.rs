use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mikrom_api::nats::NatsClient;
use mikrom_api::{AppState, create_app, repositories, scheduler};
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
    AppState {
        user_repo: Arc::new(repositories::user_repository::MockUserRepository::new()),
        app_repo: Arc::new(repositories::PostgresAppRepository::new(
            sqlx::PgPool::connect_lazy("postgres://localhost/dummy").unwrap(),
            "key".to_string(),
        )),
        volume_repo: Arc::new(repositories::volume_repository::MockVolumeRepository::new()),
        github_repo: Arc::new(repositories::MockGithubRepository::default()),
        scheduler: Arc::new(scheduler::MockScheduler::new()),
        nats: mikrom_api::nats::TypedNatsClient::new_custom(Arc::new(DummyNats)),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        jwt_secret: "test".to_string(),
        master_key: "test".to_string(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        api_db: sqlx::PgPool::connect_lazy("postgres://localhost/dummy").unwrap(),
        acme_email: "admin@mikrom.spluca.org".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        mesh_status: tokio::sync::watch::channel(mikrom_api::vms::MeshStatus::default()).0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    }
}

#[tokio::test]
async fn test_openapi_json_endpoint() {
    let app = create_app(build_state().await);

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/api-docs/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/json");

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let spec: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(spec["components"]["securitySchemes"]["jwt"]["type"], "http");
    assert_eq!(
        spec["components"]["securitySchemes"]["jwt"]["scheme"],
        "bearer"
    );

    // Verify SSE endpoint documentation
    let health_stream = &spec["paths"]["/v1/health/stream"]["get"];
    assert!(
        !health_stream.is_null(),
        "GET /v1/health/stream should be documented"
    );
    let content_type = &health_stream["responses"]["200"]["content"];
    assert!(
        content_type.get("text/event-stream").is_some(),
        "GET /v1/health/stream should have text/event-stream content type"
    );
}

#[tokio::test]
async fn test_swagger_ui_endpoint() {
    let app = create_app(build_state().await);

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/docs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/html"))
    );
}
