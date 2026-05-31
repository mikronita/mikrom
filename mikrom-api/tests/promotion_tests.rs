use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use mockall::predicate::eq;
use uuid::Uuid;

use mikrom_api::AppState;
use mikrom_api::auth::AuthUser;
use mikrom_api::domain::app::{App, Deployment};
use mikrom_api::domain::github::MockGithubRepository;
use mikrom_api::domain::user::{MockUserRepository, UserRole};
use mikrom_api::domain::{MockAppRepository, MockScheduler, MockTenantRepository};
use mikrom_api::infrastructure::http::handlers::deploy::__activate_deployment_handler_impl as activate_deployment_handler;

fn build_state(app_repo: MockAppRepository, scheduler: MockScheduler) -> AppState {
    AppState {
        ctx: mikrom_api::application::ApiContext::default(),
        user_repo: Arc::new(MockUserRepository::new()),
        tenant_repo: Arc::new(MockTenantRepository::new()),
        app_repo: Arc::new(app_repo),
        database_repo: Arc::new(mikrom_api::domain::MockDatabaseRepository::new()),
        github_repo: Arc::new(MockGithubRepository::default()),
        volume_repo: Arc::new(mikrom_api::domain::MockVolumeRepository::new()),
        scheduler: Arc::new(scheduler),
        nats: mikrom_api::nats::TypedNatsClient::default(),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        api_db: sqlx::PgPool::connect_lazy("postgres://localhost/dummy").unwrap(),
        jwt_secret: "test-secret".into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
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
async fn test_activate_deployment_promotes_running_record() {
    let tenant_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let deployment_id = Uuid::new_v4();

    let mut mock_app_repo = MockAppRepository::new();
    mock_app_repo
        .expect_get_app_by_name()
        .with(eq("test-app"))
        .returning(move |_| {
            Ok(Some(App {
                id: app_id,
                name: "test-app".to_string(),
                tenant_id,
                hostname: None,
                active_deployment_id: None,
                ..Default::default()
            }))
        });
    mock_app_repo
        .expect_get_deployment()
        .with(eq(deployment_id))
        .returning(move |_| {
            Ok(Some(Deployment {
                id: deployment_id,
                app_id,
                tenant_id,
                status: "RUNNING".to_string(),
                job_id: None,
                ..Default::default()
            }))
        });
    mock_app_repo
        .expect_set_active_deployment()
        .with(eq(app_id), eq(deployment_id))
        .times(1)
        .returning(|_, _| Ok(()));

    let scheduler = MockScheduler::new();
    let state = build_state(mock_app_repo, scheduler);
    let auth = AuthUser {
        user_id: tenant_id.to_string(),
        email: "test@example.com".to_string(),
        role: UserRole::User,
    };

    let status = activate_deployment_handler(
        auth,
        State(state),
        Path(("test-app".to_string(), deployment_id)),
    )
    .await
    .expect("handler should succeed");

    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn test_activate_failed_deployment_is_rejected() {
    let tenant_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let deployment_id = Uuid::new_v4();

    let mut mock_app_repo = MockAppRepository::new();
    mock_app_repo
        .expect_get_app_by_name()
        .with(eq("test-app"))
        .returning(move |_| {
            Ok(Some(App {
                id: app_id,
                name: "test-app".to_string(),
                tenant_id,
                hostname: None,
                active_deployment_id: None,
                ..Default::default()
            }))
        });
    mock_app_repo
        .expect_get_deployment()
        .with(eq(deployment_id))
        .returning(move |_| {
            Ok(Some(Deployment {
                id: deployment_id,
                app_id,
                tenant_id,
                status: "FAILED".to_string(),
                job_id: None,
                ..Default::default()
            }))
        });

    let scheduler = MockScheduler::new();
    let state = build_state(mock_app_repo, scheduler);
    let auth = AuthUser {
        user_id: tenant_id.to_string(),
        email: "test@example.com".to_string(),
        role: UserRole::User,
    };

    let err = activate_deployment_handler(
        auth,
        State(state),
        Path(("test-app".to_string(), deployment_id)),
    )
    .await
    .expect_err("failed deployment should be rejected");

    assert!(
        err.to_string()
            .contains("Cannot activate a failed deployment")
    );
}

#[tokio::test]
async fn test_activate_deployment_rejects_foreign_deployment() {
    let tenant_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let other_app_id = Uuid::new_v4();
    let deployment_id = Uuid::new_v4();

    let mut mock_app_repo = MockAppRepository::new();
    mock_app_repo
        .expect_get_app_by_name()
        .with(eq("test-app"))
        .returning(move |_| {
            Ok(Some(App {
                id: app_id,
                name: "test-app".to_string(),
                tenant_id,
                hostname: None,
                active_deployment_id: None,
                ..Default::default()
            }))
        });
    mock_app_repo
        .expect_get_deployment()
        .with(eq(deployment_id))
        .returning(move |_| {
            Ok(Some(Deployment {
                id: deployment_id,
                app_id: other_app_id,
                tenant_id,
                status: "RUNNING".to_string(),
                job_id: None,
                ..Default::default()
            }))
        });
    mock_app_repo.expect_set_active_deployment().times(0);

    let scheduler = MockScheduler::new();
    let state = build_state(mock_app_repo, scheduler);
    let auth = AuthUser {
        user_id: tenant_id.to_string(),
        email: "test@example.com".to_string(),
        role: UserRole::User,
    };

    let err = activate_deployment_handler(
        auth,
        State(state),
        Path(("test-app".to_string(), deployment_id)),
    )
    .await
    .expect_err("foreign deployment should be rejected");

    assert_eq!(
        err.to_string(),
        "Bad request: Deployment does not belong to this application"
    );
}
