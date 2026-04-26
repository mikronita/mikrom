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
use mikrom_api::models::app::{App, Deployment};
use mikrom_api::repositories::{MockAppRepository, MockUserRepository};

#[tokio::test]
async fn test_activate_deployment_endpoint() {
    let mock_user_repo = MockUserRepository::new();
    let mut mock_app_repo = MockAppRepository::new();

    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let deployment_id = Uuid::new_v4();
    let jwt_secret = "test-secret";

    let token = mikrom_api::auth::jwt::create_token(
        &user_id.to_string(),
        "test@example.com",
        &mikrom_api::repositories::user_repository::UserRole::User,
        jwt_secret,
    )
    .unwrap();

    // 1. Mock get_app_by_name
    let app_for_get = App {
        id: app_id,
        name: "test-app".to_string(),
        git_url: "git".to_string(),
        port: 8080,
        hostname: None,
        user_id,
        github_webhook_secret: None,
        active_deployment_id: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
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
        ip_address: None,
        status: "RUNNING".to_string(),
        vcpus: 1,
        memory_mib: 256,
        disk_mib: 1024,
        port: 8080,
        env_vars: serde_json::Value::Object(serde_json::Map::new()),
        git_commit_hash: None,
        git_commit_message: None,
        git_branch: None,
        trigger_source: "manual".into(),
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

    // 4. Mock list_deployments_by_app for cleanup logic
    mock_app_repo
        .expect_list_deployments_by_app()
        .with(eq(app_id))
        .times(1)
        .returning(move |_| Ok(vec![]));

    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
        scheduler_config: Default::default(),
        builder_addr: "http://localhost:5004".into(),
        jwt_secret: jwt_secret.into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        build_semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
    };

    let router = create_app(state);

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/apps/{}/deployments/{}/activate",
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
    let mock_user_repo = MockUserRepository::new();
    let mut mock_app_repo = MockAppRepository::new();

    let user_id = Uuid::new_v4();
    let other_user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let deployment_id = Uuid::new_v4();
    let jwt_secret = "test-secret";

    let token = mikrom_api::auth::jwt::create_token(
        &user_id.to_string(),
        "test@example.com",
        &mikrom_api::repositories::user_repository::UserRole::User,
        jwt_secret,
    )
    .unwrap();

    let app = App {
        id: app_id,
        name: "test-app".to_string(),
        git_url: "git".to_string(),
        port: 8080,
        hostname: None,
        user_id: other_user_id, // Owned by someone else
        github_webhook_secret: None,
        active_deployment_id: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    mock_app_repo
        .expect_get_app_by_name()
        .returning(move |_| Ok(Some(app.clone())));

    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
        scheduler_config: Default::default(),
        builder_addr: "http://localhost:5004".into(),
        jwt_secret: jwt_secret.into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        build_semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
    };

    let response = create_app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/apps/{}/deployments/{}/activate",
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
    let mock_user_repo = MockUserRepository::new();
    let mut mock_app_repo = MockAppRepository::new();

    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let deployment_id = Uuid::new_v4();
    let jwt_secret = "test-secret";

    let token = mikrom_api::auth::jwt::create_token(
        &user_id.to_string(),
        "test@example.com",
        &mikrom_api::repositories::user_repository::UserRole::User,
        jwt_secret,
    )
    .unwrap();

    let app = App {
        id: app_id,
        name: "test-app".to_string(),
        git_url: "git".to_string(),
        port: 8080,
        hostname: None,
        user_id,
        github_webhook_secret: None,
        active_deployment_id: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
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
        ip_address: None,
        status: "FAILED".to_string(), // Not RUNNING
        vcpus: 1,
        memory_mib: 256,
        disk_mib: 1024,
        port: 8080,
        env_vars: serde_json::Value::Object(serde_json::Map::new()),
        git_commit_hash: None,
        git_commit_message: None,
        git_branch: None,
        trigger_source: "manual".into(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    mock_app_repo
        .expect_get_deployment()
        .returning(move |_| Ok(Some(dep.clone())));

    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
        scheduler_config: Default::default(),
        builder_addr: "http://localhost:5004".into(),
        jwt_secret: jwt_secret.into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        build_semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
    };

    let response = create_app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/apps/{}/deployments/{}/activate",
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
