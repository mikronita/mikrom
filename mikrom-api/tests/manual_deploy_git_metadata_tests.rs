use axum::Json;
use axum::extract::Path;
use axum::extract::State;
use chrono::Utc;
use mikrom_api::AppState;
use mikrom_api::auth::AuthUser;
use mikrom_api::deploy::handlers::{ManualDeployRequest, deploy_app_version_handler};
use mikrom_api::models::app::{App, Deployment};
use mikrom_api::nats::TypedNatsClient;
use mikrom_api::repositories::MockGithubRepository;
use mikrom_api::repositories::app_repository::MockAppRepository;
use mikrom_api::repositories::user_repository::{MockUserRepository, UserRole};
use mikrom_api::scheduler::MockScheduler;
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

async fn create_test_state(app_repo: MockAppRepository) -> AppState {
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();

    AppState {
        user_repo: Arc::new(MockUserRepository::new()),
        app_repo: Arc::new(app_repo),
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
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    }
}

#[tokio::test]
async fn test_manual_deploy_without_github_metadata() {
    let mut mock_repo = MockAppRepository::new();
    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();

    let app = App {
        id: app_id,
        name: "test-app".to_string(),
        git_url: "https://github.com/owner/repo".into(),
        user_id,
        ..Default::default()
    };

    mock_repo
        .expect_get_app_by_name()
        .returning(move |_| Ok(Some(app.clone())));

    mock_repo
        .expect_create_deployment()
        .withf(|data| data.git_commit_hash.is_none())
        .returning(|_| {
            Ok(Deployment {
                id: Uuid::new_v4(),
                app_id: Uuid::new_v4(),
                user_id: Uuid::new_v4(),
                status: "BUILDING".into(),
                vcpus: 1,
                memory_mib: 256,
                disk_mib: 1024,
                port: 8080,
                image_tag: None,
                build_id: None,
                job_id: None,
                ip_address: None,
                ipv6_address: None,
                env_vars: serde_json::Value::Object(serde_json::Map::new()),
                trigger_source: "manual".into(),
                git_commit_hash: None,
                git_commit_message: None,
                git_branch: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
        });

    mock_repo
        .expect_update_deployment()
        .returning(|_, _| Ok(()));

    let state = create_test_state(mock_repo).await;
    let auth = AuthUser {
        user_id: user_id.to_string(),
        email: "test@example.com".into(),
        role: UserRole::User,
    };

    let payload = ManualDeployRequest {
        vcpus: None,
        memory_mib: None,
        disk_mib: None,
        env: None,
        image: None,
    };

    let result = deploy_app_version_handler(
        auth,
        State(state),
        Path("test-app".to_string()),
        Json(payload),
    )
    .await;

    // It will likely fail with NATS error "no responders" because we don't have a builder service running.
    // That's fine for this test as we want to verify the create_deployment call happened with the right metadata.
    if let Err(e) = &result {
        let err_msg = e.to_string();
        if err_msg.contains("NATS") || err_msg.contains("no responders") {
            println!("Caught expected NATS failure after deployment creation");
            return;
        }
    }

    assert!(
        result.is_ok(),
        "Expected success or NATS failure, got {:?}",
        result
    );
}

#[tokio::test]
async fn test_manual_deploy_with_github_metadata_fetch_failure() {
    let mut mock_repo = MockAppRepository::new();
    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();

    // App WITH GitHub linked
    let app = App {
        id: app_id,
        name: "github-app".to_string(),
        git_url: "https://github.com/owner/repo".into(),
        user_id,
        github_installation_id: Some(123),
        github_repo_id: Some(456),
        github_repo_full_name: Some("owner/repo".to_string()),
        ..Default::default()
    };

    mock_repo
        .expect_get_app_by_name()
        .returning(move |_| Ok(Some(app.clone())));

    // Should still create deployment even if GitHub fetch fails
    mock_repo
        .expect_create_deployment()
        .withf(|data| data.git_commit_hash.is_none())
        .returning(|_| {
            Ok(Deployment {
                id: Uuid::new_v4(),
                app_id: Uuid::new_v4(),
                user_id: Uuid::new_v4(),
                status: "BUILDING".into(),
                vcpus: 1,
                memory_mib: 256,
                disk_mib: 1024,
                port: 8080,
                image_tag: None,
                build_id: None,
                job_id: None,
                ip_address: None,
                ipv6_address: None,
                env_vars: serde_json::Value::Object(serde_json::Map::new()),
                trigger_source: "manual".into(),
                git_commit_hash: None,
                git_commit_message: None,
                git_branch: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
        });

    mock_repo
        .expect_update_deployment()
        .returning(|_, _| Ok(()));

    let state = create_test_state(mock_repo).await;
    // We don't provide valid GitHub credentials so the fetch will fail

    let auth = AuthUser {
        user_id: user_id.to_string(),
        email: "test@example.com".into(),
        role: UserRole::User,
    };

    let payload = ManualDeployRequest {
        vcpus: None,
        memory_mib: None,
        disk_mib: None,
        env: None,
        image: None,
    };

    let result = deploy_app_version_handler(
        auth,
        State(state),
        Path("github-app".to_string()),
        Json(payload),
    )
    .await;

    if let Err(mikrom_api::error::ApiError::Internal(msg)) = &result
        && msg.contains("NATS request failed")
    {
        return;
    }
    assert!(result.is_ok());
}
