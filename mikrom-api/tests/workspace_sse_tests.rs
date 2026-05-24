use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mikrom_api::AppState;
use mikrom_api::auth::jwt::create_token;
use mikrom_api::create_app;
use mikrom_api::domain::MockAppRepository;
use mikrom_api::domain::user::{MockUserRepository, UserRole};
use mikrom_api::workspace::{WorkspaceEvent, WorkspaceEventKind};
use std::sync::Arc;
use tokio_stream::StreamExt;
use tower::Service;
use uuid::Uuid;

const JWT_SECRET: &str = "test-secret";

async fn setup_app() -> (
    axum::Router,
    tokio::sync::broadcast::Sender<WorkspaceEvent>,
    Uuid,
) {
    let mock_user_repo = MockUserRepository::new();
    let mock_app_repo = MockAppRepository::new();
    let (workspace_events, _) = tokio::sync::broadcast::channel(100);
    let user_id = Uuid::new_v4();

    struct DummyNats;
    #[async_trait::async_trait]
    impl mikrom_api::nats::NatsClient for DummyNats {
        async fn request_raw(&self, _s: String, _p: Vec<u8>) -> anyhow::Result<Vec<u8>> {
            Err(anyhow::anyhow!("NATS not implemented in this test"))
        }
        async fn publish_raw(&self, _s: String, _p: Vec<u8>) -> anyhow::Result<()> {
            Ok(())
        }
        async fn subscribe_raw(&self, _s: String) -> anyhow::Result<async_nats::Subscriber> {
            Err(anyhow::anyhow!("NATS not implemented in this test"))
        }
    }
    let nats_client = mikrom_api::nats::TypedNatsClient::new_custom(Arc::new(DummyNats));

    let state = AppState {
        ctx: mikrom_api::application::ApiContext::default(),
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        volume_repo: Arc::new(mikrom_api::domain::MockVolumeRepository::new()),
        scheduler: Arc::new(mikrom_api::domain::MockScheduler::new()),
        nats: nats_client,
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        api_db: sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/dummy")
            .unwrap(),
        jwt_secret: JWT_SECRET.into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        acme_email: "admin@mikrom.spluca.org".into(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_repo: Arc::new(mikrom_api::domain::github::MockGithubRepository::default()),
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: workspace_events.clone(),
        mesh_status: tokio::sync::watch::channel(
            mikrom_api::application::vms::MeshStatus::default(),
        )
        .0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    (create_app(state), workspace_events, user_id)
}

#[tokio::test]
async fn test_workspace_events_stream() {
    let (mut router, tx, user_id) = setup_app().await;
    let token = create_token(
        &user_id.to_string(),
        "test@test.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    let req = Request::builder()
        .method("GET")
        .uri("/v1/workspace/events")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.call(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let mut body_stream = response.into_body().into_data_stream();

    // Trigger an event
    tx.send(WorkspaceEvent {
        kind: WorkspaceEventKind::AppCreated,
        user_id: Some(user_id),
        app_id: Some(Uuid::new_v4()),
        app_name: Some("test-app".to_string()),
        deployment_id: None,
        volume_id: None,
        resource_id: None,
    })
    .unwrap();

    // Receive update
    let chunk = body_stream.next().await.unwrap().unwrap();
    let chunk_str = String::from_utf8_lossy(&chunk);
    assert!(chunk_str.contains("app_created"));
    assert!(chunk_str.contains("test-app"));
}

#[tokio::test]
async fn test_workspace_events_volume_changed() {
    let (mut router, tx, user_id) = setup_app().await;
    let token = create_token(
        &user_id.to_string(),
        "test@test.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    let req = Request::builder()
        .method("GET")
        .uri("/v1/workspace/events")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.call(req).await.unwrap();
    let mut body_stream = response.into_body().into_data_stream();

    // Trigger volume changed event
    let volume_id = Uuid::new_v4();
    tx.send(WorkspaceEvent {
        kind: WorkspaceEventKind::VolumeChanged,
        user_id: Some(user_id),
        app_id: Some(Uuid::new_v4()),
        app_name: None,
        deployment_id: None,
        volume_id: Some(volume_id),
        resource_id: Some(volume_id.to_string()),
    })
    .unwrap();

    // Receive update
    let chunk = body_stream.next().await.unwrap().unwrap();
    let chunk_str = String::from_utf8_lossy(&chunk);
    assert!(chunk_str.contains("volume_changed"));
    assert!(chunk_str.contains(&volume_id.to_string()));
}

#[tokio::test]
async fn test_workspace_events_app_created() {
    let (mut router, tx, user_id) = setup_app().await;
    let token = create_token(
        &user_id.to_string(),
        "test@test.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    let req = Request::builder()
        .method("GET")
        .uri("/v1/workspace/events")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.call(req).await.unwrap();
    let mut body_stream = response.into_body().into_data_stream();

    // Trigger app created event
    let app_id = Uuid::new_v4();
    tx.send(WorkspaceEvent {
        kind: WorkspaceEventKind::AppCreated,
        user_id: Some(user_id),
        app_id: Some(app_id),
        app_name: Some("new-app".to_string()),
        deployment_id: None,
        volume_id: None,
        resource_id: None,
    })
    .unwrap();

    // Receive update
    let chunk = body_stream.next().await.unwrap().unwrap();
    let chunk_str = String::from_utf8_lossy(&chunk);
    assert!(chunk_str.contains("app_created"));
    assert!(chunk_str.contains("new-app"));
}
