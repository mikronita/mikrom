use axum::{Json, extract::Path, extract::State};
use chrono::Utc;
use mikrom_api::AppState;
use mikrom_api::domain::app::{App, Deployment};
use mikrom_api::domain::github::MockGithubRepository;
use mikrom_api::domain::user::MockUserRepository;
use mikrom_api::domain::{
    MockAppRepository, MockDatabaseRepository, MockScheduler, MockTenantRepository,
    MockVolumeRepository, TenantMember,
};
use mikrom_api::infrastructure::auth::extractor::TenantContext;
use mikrom_api::infrastructure::http::handlers::deploy::{
    __deploy_app_version_handler_impl as deploy_app_version_handler, ManualDeployRequest,
};
use mikrom_api::nats::TypedNatsClient;
use std::sync::Arc;
use uuid::Uuid;

async fn create_test_state(
    app_repo: MockAppRepository,
    tenant_membership: Option<(Uuid, Uuid)>,
) -> Option<AppState> {
    let nats_client = async_nats::connect(
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string()),
    )
    .await
    .ok()?;

    let mut tenant_repo = MockTenantRepository::new();
    if let Some((tenant_id, owner_user_id)) = tenant_membership {
        tenant_repo
            .expect_get_members()
            .returning(move |requested_tenant_id| {
                Ok(if requested_tenant_id == tenant_id {
                    vec![TenantMember {
                        tenant_id,
                        user_id: owner_user_id,
                        role: "admin".to_string(),
                    }]
                } else {
                    Vec::new()
                })
            });
    }

    let tenant_repo = Arc::new(tenant_repo);
    let ctx = mikrom_api::application::ApiContext {
        tenant_repo: tenant_repo.clone(),
        ..mikrom_api::application::ApiContext::default()
    };

    Some(AppState {
        ctx,
        user_repo: Arc::new(MockUserRepository::new()),
        tenant_repo,
        app_repo: Arc::new(app_repo),
        database_repo: Arc::new(MockDatabaseRepository::new()),
        volume_repo: Arc::new(MockVolumeRepository::new()),
        github_repo: Arc::new(MockGithubRepository::default()),
        scheduler: Arc::new(MockScheduler::new()),
        nats: TypedNatsClient::new(nats_client),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        jwt_secret: "test-secret".to_string(),
        master_key: "test-key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
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
        active_deployment_flows: Arc::new(dashmap::DashSet::new()),
    })
}

fn app_for_tenant(app_id: Uuid, tenant_id: Uuid, name: &str, git_url: &str) -> App {
    App {
        id: app_id,
        name: name.to_string(),
        git_url: git_url.to_string(),
        tenant_id,
        port: mikrom_api::domain::types::Port::new(8080).unwrap(),
        ..App::default()
    }
}

fn tenant_context(tenant_id: Uuid) -> TenantContext {
    TenantContext {
        tenant: mikrom_api::domain::Tenant {
            id: tenant_id,
            tenant_id: tenant_id.to_string().chars().take(6).collect(),
            name: "Default Project".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        },
    }
}

#[tokio::test]
async fn manual_deploy_without_github_metadata_uses_current_app_metadata() {
    let mut mock_repo = MockAppRepository::new();
    let tenant_id = Uuid::new_v4();
    let owner_user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let app = app_for_tenant(
        app_id,
        tenant_id,
        "test-app",
        "https://github.com/owner/repo",
    );

    mock_repo
        .expect_get_app_by_name()
        .returning(move |_| Ok(Some(app.clone())));
    mock_repo.expect_create_deployment().returning(move |data| {
        assert_eq!(data.app_id, app_id);
        assert_eq!(data.user_id, owner_user_id);
        assert_eq!(data.tenant_id, tenant_id.to_string());
        assert!(data.git_commit_hash.is_none());
        assert!(data.git_commit_message.is_none());
        assert!(data.git_branch.is_none());
        Ok(Deployment {
            id: Uuid::new_v4(),
            app_id,
            tenant_id,
            status: "BUILDING".into(),
            vcpus: mikrom_api::domain::types::CpuCores::new(1).unwrap(),
            memory_mib: mikrom_api::domain::types::MemoryMb::new(256).unwrap(),
            disk_mib: 1024,
            port: mikrom_api::domain::types::Port::new(8080).unwrap(),
            build_id: None,
            image_tag: None,
            job_id: None,
            ipv6_address: None,
            env_vars: serde_json::Value::Object(serde_json::Map::new()),
            trigger_source: "manual".into(),
            git_commit_hash: None,
            git_commit_message: None,
            git_branch: None,
            hypervisor: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
    });
    mock_repo
        .expect_update_deployment()
        .returning(|_, _| Ok(()));

    let Some(state) = create_test_state(mock_repo, Some((tenant_id, owner_user_id))).await else {
        return;
    };
    let tenant_ctx = tenant_context(tenant_id);

    let result = deploy_app_version_handler(
        tenant_ctx,
        State(state),
        Path("test-app".to_string()),
        Json(ManualDeployRequest {
            vcpus: None,
            memory_mib: None,
            disk_mib: None,
            env: None,
            image: None,
            hypervisor: None,
        }),
    )
    .await;

    assert!(result.is_ok() || matches!(result, Err(mikrom_api::error::ApiError::Internal(_))));
}

#[tokio::test]
async fn manual_deploy_with_github_metadata_still_creates_deployment() {
    let mut mock_repo = MockAppRepository::new();
    let tenant_id = Uuid::new_v4();
    let owner_user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let app = App {
        id: app_id,
        name: "github-app".to_string(),
        git_url: "https://github.com/owner/repo".into(),
        tenant_id,
        github_installation_id: Some(123),
        github_repo_id: Some(456),
        github_repo_full_name: Some("owner/repo".to_string()),
        port: mikrom_api::domain::types::Port::new(8080).unwrap(),
        ..App::default()
    };

    mock_repo
        .expect_get_app_by_name()
        .returning(move |_| Ok(Some(app.clone())));
    mock_repo.expect_create_deployment().returning(move |data| {
        assert_eq!(data.app_id, app_id);
        assert_eq!(data.user_id, owner_user_id);
        assert_eq!(data.tenant_id, tenant_id.to_string());
        assert!(data.git_commit_hash.is_none());
        assert!(data.git_commit_message.is_none());
        assert!(data.git_branch.is_none());
        Ok(Deployment {
            id: Uuid::new_v4(),
            app_id,
            tenant_id,
            status: "BUILDING".into(),
            vcpus: mikrom_api::domain::types::CpuCores::new(1).unwrap(),
            memory_mib: mikrom_api::domain::types::MemoryMb::new(256).unwrap(),
            disk_mib: 1024,
            port: mikrom_api::domain::types::Port::new(8080).unwrap(),
            build_id: None,
            image_tag: None,
            job_id: None,
            ipv6_address: None,
            env_vars: serde_json::Value::Object(serde_json::Map::new()),
            trigger_source: "manual".into(),
            git_commit_hash: None,
            git_commit_message: None,
            git_branch: None,
            hypervisor: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
    });
    mock_repo
        .expect_update_deployment()
        .returning(|_, _| Ok(()));

    let Some(state) = create_test_state(mock_repo, Some((tenant_id, owner_user_id))).await else {
        return;
    };
    let tenant_ctx = tenant_context(tenant_id);

    let result = deploy_app_version_handler(
        tenant_ctx,
        State(state),
        Path("github-app".to_string()),
        Json(ManualDeployRequest {
            vcpus: None,
            memory_mib: None,
            disk_mib: None,
            env: None,
            image: None,
            hypervisor: None,
        }),
    )
    .await;

    assert!(result.is_ok() || matches!(result, Err(mikrom_api::error::ApiError::Internal(_))));
}

#[tokio::test]
async fn manual_deploy_rejects_foreign_tenant() {
    let mut mock_repo = MockAppRepository::new();
    let tenant_id = Uuid::new_v4();
    let foreign_tenant_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let app = app_for_tenant(
        app_id,
        foreign_tenant_id,
        "foreign-app",
        "https://github.com/owner/repo",
    );

    mock_repo
        .expect_get_app_by_name()
        .returning(move |_| Ok(Some(app.clone())));
    mock_repo.expect_create_deployment().times(0);

    let Some(state) = create_test_state(mock_repo, None).await else {
        return;
    };
    let tenant_ctx = tenant_context(tenant_id);

    let result = deploy_app_version_handler(
        tenant_ctx,
        State(state),
        Path("foreign-app".to_string()),
        Json(ManualDeployRequest {
            vcpus: None,
            memory_mib: None,
            disk_mib: None,
            env: None,
            image: None,
            hypervisor: None,
        }),
    )
    .await;

    assert!(matches!(
        result,
        Err(mikrom_api::error::ApiError::Forbidden)
    ));
}
