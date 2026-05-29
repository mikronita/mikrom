mod common;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::Utc;
use mockall::predicate::*;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

use mikrom_api::AppState;
use mikrom_api::create_app;
use mikrom_api::domain::app::{App, Deployment};
use mikrom_api::domain::user::{User, UserRole};
use mikrom_api::domain::{MockAppRepository, MockUserRepository};
use mikrom_api::test_utils::TestDb;

#[tokio::test]
async fn test_activate_deployment_endpoint() {
    let mut mock_user_repo = MockUserRepository::new();
    mock_user_repo.expect_find_by_id().returning(|id| {
        Ok(Some(User {
            id,
            email: "test@example.com".into(),
            password_hash: "hash".into(),
            role: UserRole::User,
            first_name: None,
            last_name: None,
            vpc_ipv6_prefix: None,
        }))
    });
    let mut mock_app_repo = MockAppRepository::new();
    let db = TestDb::new().await;
    let db_pool = db.pool().clone();

    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let deployment_id = Uuid::new_v4();
    let jwt_secret = "test-secret";

    let token = mikrom_api::auth::jwt::create_token(
        &user_id.to_string(),
        "test@example.com",
        &mikrom_api::domain::user::UserRole::User,
        jwt_secret,
    )
    .unwrap();

    // ... (rest of the test)
    // 1. Mock get_app_by_name
    let app_for_get = App {
        id: app_id,
        name: "test-app".to_string(),
        user_id,
        ..Default::default()
    };
    mock_app_repo
        .expect_get_app_by_name()
        .with(eq("test-app"))
        .times(1)
        .returning(move |_| Ok(Some(app_for_get.clone())));

    // 2. Mock get_deployment
    let dep_for_get = Deployment {
        id: deployment_id,
        app_id,
        user_id,
        build_id: None,
        image_tag: None,
        job_id: None,
        ipv6_address: None,
        status: "RUNNING".to_string(),
        vcpus: mikrom_api::domain::types::CpuCores::new(1).unwrap(),
        memory_mib: mikrom_api::domain::types::MemoryMb::new(256).unwrap(),
        disk_mib: 1024,
        port: mikrom_api::domain::types::Port::new(8080).unwrap(),
        env_vars: serde_json::Value::Object(serde_json::Map::new()),
        git_commit_hash: None,
        git_commit_message: None,
        git_branch: None,
        trigger_source: "manual".into(),
        hypervisor: 0,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    mock_app_repo
        .expect_get_deployment()
        .with(eq(deployment_id))
        .times(1)
        .returning(move |_| Ok(Some(dep_for_get.clone())));

    // 3. Mock set_active_deployment
    mock_app_repo
        .expect_set_active_deployment()
        .with(eq(app_id), eq(deployment_id))
        .times(1)
        .returning(|_, _| Ok(()));

    // 4. Mock get_app for notify_router
    let app_for_notify = App {
        id: app_id,
        name: "test-app".to_string(),
        user_id,
        active_deployment_id: Some(deployment_id),
        ..Default::default()
    };
    let app_for_notify_clone = app_for_notify.clone();
    mock_app_repo
        .expect_get_app()
        .with(eq(app_id))
        .returning(move |_| Ok(Some(app_for_notify_clone.clone())));

    // 5. Mock list_deployments_by_app for cleanup logic (no longer called by handler if job_id is present)
    mock_app_repo
        .expect_list_deployments_by_app()
        .with(eq(app_id))
        .returning(move |_| Ok(vec![]));
    let Some(nats_client) = common::get_nats_client_or_skip().await else {
        return;
    };
    let state = AppState {
        ctx: mikrom_api::application::ApiContext::default(),
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        database_repo: Arc::new(mikrom_api::domain::MockDatabaseRepository::new()),
        volume_repo: Arc::new(mikrom_api::domain::MockVolumeRepository::new()),
        scheduler: Arc::new(mikrom_api::domain::MockScheduler::new()),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        jwt_secret: jwt_secret.into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        api_db: db_pool,
        acme_email: "admin@mikrom.spluca.org".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_repo: Arc::new(mikrom_api::domain::github::MockGithubRepository::default()),
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        mesh_status: tokio::sync::watch::channel(
            mikrom_api::application::vms::MeshStatus::default(),
        )
        .0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    let router = create_app(state);

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/apps/{}/deployments/{}/activate",
                    "test-app", deployment_id
                ))
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_activate_deployment_wrong_owner() {
    let mut mock_user_repo = MockUserRepository::new();
    mock_user_repo.expect_find_by_id().returning(|id| {
        Ok(Some(User {
            id,
            email: "test@example.com".into(),
            password_hash: "hash".into(),
            role: UserRole::User,
            first_name: None,
            last_name: None,
            vpc_ipv6_prefix: None,
        }))
    });
    let mut mock_app_repo = MockAppRepository::new();

    let user_id = Uuid::new_v4();
    let other_user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let deployment_id = Uuid::new_v4();
    let jwt_secret = "test-secret";

    let token = mikrom_api::auth::jwt::create_token(
        &user_id.to_string(),
        "test@example.com",
        &mikrom_api::domain::user::UserRole::User,
        jwt_secret,
    )
    .unwrap();

    let app = App {
        id: app_id,
        name: "test-app".to_string(),
        user_id: other_user_id, // Owned by someone else
        ..Default::default()
    };
    mock_app_repo
        .expect_get_app_by_name()
        .returning(move |_| Ok(Some(app.clone())));

    let db = TestDb::new().await;
    let db_pool = db.pool().clone();
    let Some(nats_client) = common::get_nats_client_or_skip().await else {
        return;
    };
    let state = AppState {
        ctx: mikrom_api::application::ApiContext::default(),
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        database_repo: Arc::new(mikrom_api::domain::MockDatabaseRepository::new()),
        volume_repo: Arc::new(mikrom_api::domain::MockVolumeRepository::new()),
        scheduler: Arc::new(mikrom_api::domain::MockScheduler::new()),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        jwt_secret: jwt_secret.into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        api_db: db_pool,
        acme_email: "admin@mikrom.spluca.org".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_repo: Arc::new(mikrom_api::domain::github::MockGithubRepository::default()),
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        mesh_status: tokio::sync::watch::channel(
            mikrom_api::application::vms::MeshStatus::default(),
        )
        .0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    let response = create_app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/apps/{}/deployments/{}/activate",
                    "test-app", deployment_id
                ))
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_activate_deployment_not_running() {
    let mut mock_user_repo = MockUserRepository::new();
    mock_user_repo.expect_find_by_id().returning(|id| {
        Ok(Some(User {
            id,
            email: "test@example.com".into(),
            password_hash: "hash".into(),
            role: UserRole::User,
            first_name: None,
            last_name: None,
            vpc_ipv6_prefix: None,
        }))
    });
    let mut mock_app_repo = MockAppRepository::new();

    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let deployment_id = Uuid::new_v4();
    let jwt_secret = "test-secret";

    let token = mikrom_api::auth::jwt::create_token(
        &user_id.to_string(),
        "test@example.com",
        &mikrom_api::domain::user::UserRole::User,
        jwt_secret,
    )
    .unwrap();

    let app = App {
        id: app_id,
        name: "test-app".to_string(),
        user_id, // This one is user_id
        ..Default::default()
    };
    mock_app_repo
        .expect_get_app_by_name()
        .returning(move |_| Ok(Some(app.clone())));

    let dep = Deployment {
        id: deployment_id,
        app_id,
        user_id,
        build_id: None,
        image_tag: None,
        job_id: None,
        ipv6_address: None,
        status: "FAILED".to_string(), // Not RUNNING
        vcpus: mikrom_api::domain::types::CpuCores::new(1).unwrap(),
        memory_mib: mikrom_api::domain::types::MemoryMb::new(256).unwrap(),
        disk_mib: 1024,
        port: mikrom_api::domain::types::Port::new(8080).unwrap(),
        env_vars: serde_json::Value::Object(serde_json::Map::new()),
        git_commit_hash: None,
        git_commit_message: None,
        git_branch: None,
        trigger_source: "manual".into(),
        hypervisor: 0,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    mock_app_repo
        .expect_get_deployment()
        .returning(move |_| Ok(Some(dep.clone())));

    mock_app_repo
        .expect_list_deployments_by_app()
        .returning(|_| Ok(vec![]));

    mock_app_repo
        .expect_set_active_deployment()
        .returning(|_, _| Ok(()));

    let db = TestDb::new().await;
    let db_pool = db.pool().clone();
    let Some(nats_client) = common::get_nats_client_or_skip().await else {
        return;
    };
    let state = AppState {
        ctx: mikrom_api::application::ApiContext::default(),
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        database_repo: Arc::new(mikrom_api::domain::MockDatabaseRepository::new()),
        volume_repo: Arc::new(mikrom_api::domain::MockVolumeRepository::new()),
        scheduler: Arc::new(mikrom_api::domain::MockScheduler::new()),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        jwt_secret: jwt_secret.into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        api_db: db_pool,
        acme_email: "admin@mikrom.spluca.org".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_repo: Arc::new(mikrom_api::domain::github::MockGithubRepository::default()),
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        mesh_status: tokio::sync::watch::channel(
            mikrom_api::application::vms::MeshStatus::default(),
        )
        .0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    let response = create_app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/apps/{}/deployments/{}/activate",
                    "test-app", deployment_id
                ))
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
