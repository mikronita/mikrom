use mikrom_api::AppState;
use mikrom_api::deploy::worker::{
    BuildTask, MockBuilderClient, MockSchedulerClient, poll_and_deploy,
};
use mikrom_api::models::app::{App, Deployment};
use mikrom_api::nats::TypedNatsClient;
use mikrom_api::repositories::MockGithubRepository;
use mikrom_api::repositories::app_repository::MockAppRepository;
use mikrom_api::repositories::user_repository::MockUserRepository;
use mikrom_api::scheduler::MockScheduler;
use mikrom_proto::builder::BuildStatus;
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

async fn create_test_state(
    app_repo: MockAppRepository,
    user_repo: MockUserRepository,
    volume_repo: mikrom_api::repositories::volume_repository::MockVolumeRepository,
) -> AppState {
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();

    AppState {
        user_repo: Arc::new(user_repo),
        app_repo: Arc::new(app_repo),
        volume_repo: Arc::new(volume_repo),
        github_repo: Arc::new(MockGithubRepository::default()),
        scheduler: Arc::new(MockScheduler::new()),
        nats: TypedNatsClient::new(nats_client),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        jwt_secret: "test-secret".to_string(),
        master_key: "test-key".into(),
        deployment_events: broadcast::channel(1).0,
        api_db: sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/dummy")
            .unwrap(),
        acme_email: "test@example.com".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: Some("123".to_string()),
        github_private_key: Some("dummy-key".to_string()),
        github_app_slug: Some("test-app".to_string()),
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        mesh_status: tokio::sync::watch::channel(mikrom_api::vms::MeshStatus::default()).0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    }
}

#[tokio::test]
async fn test_port_propagation_from_builder_to_deployment() {
    let mut mock_repo = MockAppRepository::new();
    let mut mock_user_repo = MockUserRepository::new();
    let mut mock_volume_repo =
        mikrom_api::repositories::volume_repository::MockVolumeRepository::new();
    let mut mock_builder = MockBuilderClient::new();
    let _mock_scheduler = MockSchedulerClient::new();

    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let deployment_id = Uuid::new_v4();
    let build_id = "test-build-123".to_string();

    let app = App {
        id: app_id,
        name: "test-app".to_string(),
        git_url: "https://github.com/owner/repo".into(),
        user_id,
        port: 8080, // Default port
        ..Default::default()
    };

    let deployment = Deployment {
        id: deployment_id,
        app_id,
        user_id,
        status: "BUILDING".into(),
        port: 8080,
        ..Default::default()
    };

    // 1. Mock Repository
    let app_clone = app.clone();
    mock_repo
        .expect_get_app()
        .returning(move |_| Ok(Some(app_clone.clone())));

    let dep_clone = deployment.clone();
    mock_repo
        .expect_get_deployment()
        .returning(move |_| Ok(Some(dep_clone.clone())));

    // Verify that the deployment port is updated to 80
    mock_repo
        .expect_update_deployment_port()
        .with(
            mockall::predicate::eq(deployment_id),
            mockall::predicate::eq(80),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    mock_repo
        .expect_update_deployment()
        .returning(|_, _| Ok(()));

    // Mock volumes
    mock_volume_repo
        .expect_list_volumes_by_app()
        .returning(|_| Ok(vec![]));

    // Mock user
    mock_user_repo.expect_find_by_id().returning(move |id| {
        Ok(Some(mikrom_api::repositories::user_repository::User {
            id,
            email: "test@example.com".into(),
            password_hash: "hash".into(),
            role: mikrom_api::repositories::user_repository::UserRole::User,
            first_name: None,
            last_name: None,
            vpc_ipv6_prefix: Some("fd00::/64".to_string()),
        }))
    });

    // 2. Mock Builder to report port 80
    mock_builder
        .expect_get_build_status()
        .with(mockall::predicate::eq(build_id.clone()))
        .returning(|_| {
            Box::pin(async move {
                Ok((
                    BuildStatus::Success,
                    "registry.mikrom.spluca.org/mikrom/test-app:latest".to_string(),
                    80, // DETECTED PORT 80
                    Some("hash".to_string()),
                    Some("msg".to_string()),
                    Some("branch".to_string()),
                ))
            })
        });

    let state = create_test_state(mock_repo, mock_user_repo, mock_volume_repo).await;

    let task = BuildTask {
        deployment_id,
        app_id,
        app_name: app.name.clone(),
        user_id: user_id.to_string(),
        build_id: build_id.clone(),
        vcpus: 1,
        memory_mib: 256,
        disk_mib: 1024,
        port: 8080, // Original port
        env: std::collections::HashMap::new(),
    };

    let result = poll_and_deploy(
        state,
        task,
        Arc::new(mock_builder),
        Arc::new(MockSchedulerClient::new()),
        None,
    )
    .await;

    // The flow may still fail later in deploy_to_scheduler, but the port update
    // check is done via MockAppRepository expectations.
    assert!(result.is_err());
}
