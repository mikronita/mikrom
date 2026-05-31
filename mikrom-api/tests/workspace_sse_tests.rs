use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mikrom_api::auth::jwt::create_token;
use mikrom_api::create_app;
use mikrom_api::domain::user::{MockUserRepository, UserRole};
use mikrom_api::domain::{
    MockAppRepository, MockDatabaseRepository, MockScheduler, MockTenantRepository,
    MockVolumeRepository,
};
use mikrom_api::workspace::{WorkspaceEvent, WorkspaceEventKind};
use mikrom_api::{AppState, nats::TypedNatsClient};
use std::sync::Arc;
use tokio::time::{Duration, timeout};
use tokio_stream::StreamExt;
use tower::ServiceExt;
use uuid::Uuid;

const JWT_SECRET: &str = "test-secret";

struct DummyNats;

#[async_trait::async_trait]
impl mikrom_api::nats::NatsClient for DummyNats {
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

#[allow(clippy::field_reassign_with_default)]
fn build_state() -> (
    AppState,
    tokio::sync::broadcast::Sender<WorkspaceEvent>,
    Uuid,
) {
    let mut state = AppState::default();
    state.jwt_secret = JWT_SECRET.to_string();
    state.ctx.jwt_secret = state.jwt_secret.clone();
    state.master_key = "test-master-key".to_string();
    state.ctx.master_key = state.master_key.clone();
    state.user_repo = Arc::new(MockUserRepository::new());
    state.ctx.user_repo = state.user_repo.clone();
    state.tenant_repo = Arc::new(MockTenantRepository::new());
    state.ctx.tenant_repo = state.tenant_repo.clone();
    state.app_repo = Arc::new(MockAppRepository::new());
    state.ctx.app_repo = state.app_repo.clone();
    state.database_repo = Arc::new(MockDatabaseRepository::new());
    state.ctx.database_repo = state.database_repo.clone();
    state.volume_repo = Arc::new(MockVolumeRepository::new());
    state.ctx.volume_repo = state.volume_repo.clone();
    state.scheduler = Arc::new(MockScheduler::new());
    state.ctx.scheduler = state.scheduler.clone();
    state.nats = TypedNatsClient::new_custom(Arc::new(DummyNats));
    state.ctx.nats = state.nats.clone();

    let user_id = Uuid::new_v4();
    let (workspace_events, _) = tokio::sync::broadcast::channel(16);
    state.workspace_events = workspace_events.clone();

    (state, workspace_events, user_id)
}

#[tokio::test]
async fn workspace_stream_emits_matching_tenant_events() {
    let (state, tx, user_id) = build_state();
    let token = create_token(
        &user_id.to_string(),
        "test@example.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    let app = create_app(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/workspace/events")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let mut body_stream = response.into_body().into_data_stream();

    tx.send(WorkspaceEvent {
        kind: WorkspaceEventKind::AppCreated,
        user_id: Some(user_id),
        tenant_id: Some(user_id),
        app_id: Some(Uuid::new_v4()),
        app_name: Some("tenant-app".to_string()),
        deployment_id: None,
        volume_id: None,
        resource_id: None,
    })
    .unwrap();

    let chunk = body_stream.next().await.unwrap().unwrap();
    let chunk_str = String::from_utf8_lossy(&chunk);
    assert!(chunk_str.contains("app_created"));
    assert!(chunk_str.contains("tenant-app"));
}

#[tokio::test]
async fn workspace_stream_ignores_other_tenant_events() {
    let (state, tx, user_id) = build_state();
    let other_tenant_id = Uuid::new_v4();
    let token = create_token(
        &user_id.to_string(),
        "test@example.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    let app = create_app(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/workspace/events")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let mut body_stream = response.into_body().into_data_stream();

    tx.send(WorkspaceEvent {
        kind: WorkspaceEventKind::AppCreated,
        user_id: Some(other_tenant_id),
        tenant_id: Some(other_tenant_id),
        app_id: Some(Uuid::new_v4()),
        app_name: Some("foreign-app".to_string()),
        deployment_id: None,
        volume_id: None,
        resource_id: None,
    })
    .unwrap();

    let next_event = timeout(Duration::from_millis(150), body_stream.next()).await;
    assert!(
        next_event.is_err(),
        "foreign tenant event should be filtered out"
    );
}
