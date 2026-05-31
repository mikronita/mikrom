use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use mockall::predicate::eq;
use tokio::time::{Duration, timeout};
use tokio_stream::StreamExt;
use tower::ServiceExt;

use mikrom_api::AppState;
use mikrom_api::auth::jwt::create_token;
use mikrom_api::create_app;
use mikrom_api::domain::app::{App, Deployment};
use mikrom_api::domain::github::MockGithubRepository;
use mikrom_api::domain::user::{MockUserRepository, UserRole};
use mikrom_api::domain::{MockAppRepository, MockScheduler, MockTenantRepository};

const JWT_SECRET: &str = "test-secret";

async fn connect_nats_or_skip() -> Option<async_nats::Client> {
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());

    match async_nats::connect(nats_url).await {
        Ok(client) => Some(client),
        Err(err) => {
            eprintln!("skipping SSE deployment test: unable to connect to NATS: {err}");
            None
        },
    }
}

async fn build_state(
    app_repo: MockAppRepository,
    _tenant_id: uuid::Uuid,
    nats_client: async_nats::Client,
) -> AppState {
    let mut scheduler = MockScheduler::new();
    scheduler
        .expect_list_apps()
        .returning(|_| Ok(mikrom_proto::scheduler::ListAppsResponse::default()));

    AppState {
        ctx: mikrom_api::application::ApiContext::default(),
        user_repo: Arc::new(MockUserRepository::new()),
        tenant_repo: Arc::new(MockTenantRepository::new()),
        app_repo: Arc::new(app_repo),
        database_repo: Arc::new(mikrom_api::domain::MockDatabaseRepository::new()),
        github_repo: Arc::new(MockGithubRepository::default()),
        volume_repo: Arc::new(mikrom_api::domain::MockVolumeRepository::new()),
        scheduler: Arc::new(scheduler),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        api_db: sqlx::PgPool::connect_lazy("postgres://localhost/dummy").unwrap(),
        jwt_secret: JWT_SECRET.into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(100).0,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        mesh_status: tokio::sync::watch::channel(
            mikrom_api::application::vms::MeshStatus::default(),
        )
        .0,
        acme_email: "admin@mikrom.spluca.org".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    }
}

#[tokio::test]
#[ignore = "requires a stable SSE initial snapshot fixture"]
async fn test_sse_deployments_stream_initial_data() {
    let app_name = "test-app";
    let app_id = uuid::Uuid::new_v4();
    let tenant_id = uuid::Uuid::new_v4();
    let Some(nats_client) = connect_nats_or_skip().await else {
        return;
    };

    let mut mock_app_repo = MockAppRepository::new();
    mock_app_repo
        .expect_get_app_by_name()
        .with(eq(app_name))
        .returning(move |_| {
            Ok(Some(App {
                id: app_id,
                name: app_name.to_string(),
                tenant_id,
                ..Default::default()
            }))
        });
    mock_app_repo
        .expect_list_deployments_by_app()
        .with(eq(app_id))
        .returning(move |_| {
            Ok(vec![Deployment {
                id: uuid::Uuid::new_v4(),
                app_id,
                tenant_id,
                status: "RUNNING".to_string(),
                job_id: Some("job-local".to_string()),
                ..Default::default()
            }])
        });

    let state = build_state(mock_app_repo, tenant_id, nats_client).await;
    let router = create_app(state);
    let token = create_token(
        &tenant_id.to_string(),
        "test@test.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/apps/{app_name}/deployments/stream"))
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/event-stream"
    );

    let mut chunks = response.into_body().into_data_stream();
    let first_chunk = chunks.next().await.unwrap().unwrap();
    assert!(String::from_utf8_lossy(&first_chunk).contains("[]"));
}

#[tokio::test]
async fn test_sse_deployments_stream_updates() {
    let app_name = "test-app-updates";
    let app_id = uuid::Uuid::new_v4();
    let tenant_id = uuid::Uuid::new_v4();
    let Some(nats_client) = connect_nats_or_skip().await else {
        return;
    };

    let mut mock_app_repo = MockAppRepository::new();
    mock_app_repo
        .expect_get_app_by_name()
        .with(eq(app_name))
        .returning(move |_| {
            Ok(Some(App {
                id: app_id,
                name: app_name.to_string(),
                tenant_id,
                ..Default::default()
            }))
        });
    mock_app_repo
        .expect_list_deployments_by_app()
        .with(eq(app_id))
        .times(1)
        .returning(|_| Ok(vec![]));
    mock_app_repo
        .expect_list_deployments_by_app()
        .with(eq(app_id))
        .times(1)
        .returning(move |_| {
            Ok(vec![Deployment {
                id: uuid::Uuid::new_v4(),
                app_id,
                tenant_id,
                status: "RUNNING".to_string(),
                job_id: Some("job-updated".to_string()),
                ..Default::default()
            }])
        });

    let state = build_state(mock_app_repo, tenant_id, nats_client).await;
    let tx = state.deployment_events.clone();
    let router = create_app(state);
    let token = create_token(
        &tenant_id.to_string(),
        "test@test.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/apps/{app_name}/deployments/stream"))
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let mut body_stream = response.into_body().into_data_stream();

    let first_chunk = body_stream.next().await.unwrap().unwrap();
    assert!(String::from_utf8_lossy(&first_chunk).contains("[]"));

    tx.send(app_id).unwrap();

    let second_chunk = body_stream.next().await.unwrap().unwrap();
    let second_str = String::from_utf8_lossy(&second_chunk);
    assert!(second_str.contains("job-updated"));
    assert!(second_str.contains("RUNNING"));
}

#[tokio::test]
async fn test_sse_deployments_auth_via_query_param() {
    let app_name = "test-app-query-auth";
    let app_id = uuid::Uuid::new_v4();
    let tenant_id = uuid::Uuid::new_v4();
    let Some(nats_client) = connect_nats_or_skip().await else {
        return;
    };

    let mut mock_app_repo = MockAppRepository::new();
    mock_app_repo
        .expect_get_app_by_name()
        .with(eq(app_name))
        .returning(move |_| {
            Ok(Some(App {
                id: app_id,
                name: app_name.to_string(),
                tenant_id,
                ..Default::default()
            }))
        });
    mock_app_repo
        .expect_list_deployments_by_app()
        .with(eq(app_id))
        .returning(|_| Ok(vec![]));

    let state = build_state(mock_app_repo, tenant_id, nats_client).await;
    let router = create_app(state);
    let token = create_token(
        &tenant_id.to_string(),
        "test@test.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/v1/apps/{app_name}/deployments/stream?token={token}"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
#[ignore = "requires a stable SSE tenant-filter fixture"]
async fn test_sse_deployments_ignores_other_tenant_events() {
    let app_name = "test-app-filter";
    let app_id = uuid::Uuid::new_v4();
    let tenant_id = uuid::Uuid::new_v4();
    let other_tenant_id = uuid::Uuid::new_v4();
    let Some(nats_client) = connect_nats_or_skip().await else {
        return;
    };

    let mut mock_app_repo = MockAppRepository::new();
    mock_app_repo
        .expect_get_app_by_name()
        .with(eq(app_name))
        .returning(move |_| {
            Ok(Some(App {
                id: app_id,
                name: app_name.to_string(),
                tenant_id,
                ..Default::default()
            }))
        });
    mock_app_repo
        .expect_list_deployments_by_app()
        .with(eq(app_id))
        .returning(|_| Ok(vec![]));

    let state = build_state(mock_app_repo, tenant_id, nats_client.clone()).await;
    let router = create_app(state);
    let token = create_token(
        &tenant_id.to_string(),
        "test@test.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/apps/{app_name}/deployments/stream"))
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let mut body_stream = response.into_body().into_data_stream();
    let first_chunk = body_stream.next().await.unwrap().unwrap();
    assert!(String::from_utf8_lossy(&first_chunk).contains("[]"));

    use mikrom_proto::scheduler::AppInfo;
    use prost::Message;

    let foreign_job = AppInfo {
        job_id: "foreign-job".to_string(),
        deployment_id: "job-local".to_string(),
        app_id: app_id.to_string(),
        tenant_id: other_tenant_id.to_string(),
        app_name: app_name.to_string(),
        status: mikrom_proto::scheduler::DeployStatus::Running as i32,
        ipv6_address: "fd00::99".to_string(),
        ..Default::default()
    };
    let mut buf = Vec::new();
    foreign_job.encode(&mut buf).unwrap();
    nats_client
        .publish("mikrom.scheduler.job_updates", buf.into())
        .await
        .unwrap();

    let next = timeout(Duration::from_millis(200), body_stream.next()).await;
    assert!(next.is_err() || next.unwrap().is_none());
}
