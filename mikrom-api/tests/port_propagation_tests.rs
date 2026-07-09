use mikrom_api::AppState;
use mikrom_api::application::deployment::service::{DeployParams, DeploymentService};
use mikrom_api::domain::MockScheduler;
use mikrom_api::domain::Port;
use mikrom_api::domain::app::{App, Deployment};
use mikrom_api::domain::github::MockGithubRepository;
use mikrom_api::domain::user::MockUserRepository;
use mikrom_api::domain::{MockAppRepository, TenantMember};
use mikrom_api::nats::TypedNatsClient;
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

fn nats_integration_enabled() -> bool {
    if std::env::var("MIKROM_RUN_NATS_TESTS").is_err() {
        println!("Skipping NATS test: set MIKROM_RUN_NATS_TESTS=1 to run it");
        return false;
    }

    true
}

async fn connect_nats_or_skip() -> Option<async_nats::Client> {
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());

    match async_nats::connect(nats_url).await {
        Ok(client) => Some(client),
        Err(err) => {
            eprintln!(
                "skipping port propagation test: unable to connect to NATS: {}",
                err
            );
            None
        },
    }
}

async fn create_test_state(
    app_repo: MockAppRepository,
    user_repo: MockUserRepository,
    volume_repo: mikrom_api::domain::MockVolumeRepository,
) -> Option<AppState> {
    let nats_client = connect_nats_or_skip().await?;

    let mut tenant_repo = mikrom_api::domain::MockTenantRepository::new();
    tenant_repo.expect_get_members().returning(|tenant_id| {
        Ok(vec![TenantMember {
            tenant_id,
            user_id: Uuid::new_v4(),
            role: "admin".to_string(),
        }])
    });

    Some(AppState {
        ctx: mikrom_api::application::ApiContext::default(),
        user_repo: Arc::new(user_repo),
        tenant_repo: Arc::new(tenant_repo),
        app_repo: Arc::new(app_repo),
        database_repo: Arc::new(mikrom_api::domain::MockDatabaseRepository::new()),
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
        mesh_status: tokio::sync::watch::channel(
            mikrom_api::application::vms::MeshStatus::default(),
        )
        .0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    })
}

#[tokio::test]
#[ignore = "requires a stable tenant membership fixture"]
async fn test_port_propagation_from_builder_to_deployment() {
    if !nats_integration_enabled() {
        return;
    }

    let mut mock_repo = MockAppRepository::new();
    let mut mock_user_repo = MockUserRepository::new();
    let mut mock_volume_repo = mikrom_api::domain::MockVolumeRepository::new();

    let tenant_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let deployment_id = Uuid::new_v4();

    let app = App {
        id: app_id,
        name: "test-app".to_string(),
        git_url: "https://github.com/owner/repo".into(),
        tenant_id,
        port: mikrom_api::domain::types::Port::new(8080).unwrap(),
        ..Default::default()
    };

    let deployment = Deployment {
        id: deployment_id,
        app_id,
        tenant_id,
        status: "BUILDING".into(),
        port: mikrom_api::domain::types::Port::new(8080).unwrap(),
        ..Default::default()
    };

    let app_clone = app.clone();
    mock_repo
        .expect_get_app()
        .returning(move |_| Ok(Some(app_clone.clone())));

    let dep_clone = deployment.clone();
    mock_repo
        .expect_get_deployment()
        .returning(move |_| Ok(Some(dep_clone.clone())));

    mock_repo
        .expect_update_deployment_port()
        .with(
            mockall::predicate::eq(deployment_id),
            mockall::predicate::eq(Port::new(80).unwrap()),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    mock_repo
        .expect_update_deployment()
        .returning(|_, _| Ok(()));

    mock_volume_repo
        .expect_list_volumes_by_app()
        .returning(|_| Ok(vec![]));

    mock_user_repo.expect_find_by_id().returning(move |id| {
        Ok(Some(mikrom_api::domain::user::User {
            id,
            email: "test@example.com".into(),
            password_hash: "hash".into(),
            avatar_url: None,
            role: mikrom_api::domain::user::UserRole::User,
            first_name: None,
            last_name: None,
            vpc_ipv6_prefix: Some("fd00::/64".to_string()),
            totp_secret: None,
            totp_enabled: false,
            deleted_at: None,
            email_notifications: true,
            marketing_emails: false,
        }))
    });

    let Some(state) = create_test_state(mock_repo, mock_user_repo, mock_volume_repo).await else {
        return;
    };

    let result = DeploymentService::deploy_to_scheduler(
        &state,
        &app,
        &deployment,
        DeployParams {
            image_tag: "registry.mikrom.spluca.org/mikrom/test-app:latest".to_string(),
            vcpus: mikrom_api::domain::types::CpuCores::new(1).unwrap(),
            memory_mib: mikrom_api::domain::types::MemoryMb::new(256).unwrap(),
            disk_mib: 1024,
            port: Port::new(80).unwrap(),
            env: std::collections::HashMap::new(),
            hypervisor: 0,
        },
    )
    .await;

    assert!(result.is_err());
}

#[tokio::test]
#[ignore = "requires a stable tenant membership fixture"]
async fn test_zero_reported_port_keeps_original_deployment_port() {
    if !nats_integration_enabled() {
        return;
    }

    let mut mock_repo = MockAppRepository::new();
    let mut mock_user_repo = MockUserRepository::new();
    let mut mock_volume_repo = mikrom_api::domain::MockVolumeRepository::new();

    let tenant_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let deployment_id = Uuid::new_v4();

    let app = App {
        id: app_id,
        name: "test-app".to_string(),
        git_url: "https://github.com/owner/repo".into(),
        tenant_id,
        port: mikrom_api::domain::types::Port::new(8080).unwrap(),
        ..Default::default()
    };

    let deployment = Deployment {
        id: deployment_id,
        app_id,
        tenant_id,
        status: "BUILDING".into(),
        port: mikrom_api::domain::types::Port::new(8080).unwrap(),
        ..Default::default()
    };

    let app_clone = app.clone();
    mock_repo
        .expect_get_app()
        .returning(move |_| Ok(Some(app_clone.clone())));

    let dep_clone = deployment.clone();
    mock_repo
        .expect_get_deployment()
        .returning(move |_| Ok(Some(dep_clone.clone())));

    mock_repo.expect_update_deployment_port().times(0);
    mock_repo
        .expect_update_deployment()
        .returning(|_, _| Ok(()));

    mock_volume_repo
        .expect_list_volumes_by_app()
        .returning(|_| Ok(vec![]));

    mock_user_repo.expect_find_by_id().returning(move |id| {
        Ok(Some(mikrom_api::domain::user::User {
            id,
            email: "test@example.com".into(),
            password_hash: "hash".into(),
            avatar_url: None,
            role: mikrom_api::domain::user::UserRole::User,
            first_name: None,
            last_name: None,
            vpc_ipv6_prefix: Some("fd00::/64".to_string()),
            totp_secret: None,
            totp_enabled: false,
            deleted_at: None,
            email_notifications: true,
            marketing_emails: false,
        }))
    });

    let Some(state) = create_test_state(mock_repo, mock_user_repo, mock_volume_repo).await else {
        return;
    };

    let result = DeploymentService::deploy_to_scheduler(
        &state,
        &app,
        &deployment,
        DeployParams {
            image_tag: "registry.mikrom.spluca.org/mikrom/test-app:latest".to_string(),
            vcpus: mikrom_api::domain::types::CpuCores::new(1).unwrap(),
            memory_mib: mikrom_api::domain::types::MemoryMb::new(256).unwrap(),
            disk_mib: 1024,
            port: Port::new(8080).unwrap(),
            env: std::collections::HashMap::new(),
            hypervisor: 0,
        },
    )
    .await;

    assert!(result.is_err());
}
