use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::Utc;
use mockall::predicate::{self, *};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

use mikrom_api::AppState;
use mikrom_api::create_app;
use mikrom_api::models::app::{App, Deployment};
use mikrom_api::repositories::app_repository::UpdateDeploymentParams;
use mikrom_api::repositories::{MockAppRepository, MockUserRepository};
use mikrom_api::scheduler::MockScheduler;

#[tokio::test]
async fn test_promotion_back_and_forth() {
    let mock_user_repo = MockUserRepository::new();
    let mut mock_app_repo = MockAppRepository::new();
    let mut mock_scheduler = MockScheduler::new();

    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let dep1_id = Uuid::new_v4();
    let dep2_id = Uuid::new_v4();
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
        active_deployment_id: Some(dep1_id),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    // We simulate activating dep2.
    let app_for_mock = app.clone();
    mock_app_repo
        .expect_get_app_by_name()
        .returning(move |_| Ok(Some(app_for_mock.clone())));

    let dep2 = Deployment {
        id: dep2_id,
        app_id,
        user_id,
        status: "STOPPED".to_string(),
        job_id: Some("job-2".to_string()),
        ..Default::default()
    };

    let dep2_clone = dep2.clone();
    mock_app_repo
        .expect_get_deployment()
        .with(eq(dep2_id))
        .returning(move |_| Ok(Some(dep2_clone.clone())));

    // Mock get_app
    let app_clone = app.clone();
    mock_app_repo
        .expect_get_app()
        .with(eq(app_id))
        .returning(move |_| Ok(Some(app_clone.clone())));

    mock_app_repo
        .expect_set_active_deployment()
        .with(eq(app_id), eq(dep2_id))
        .returning(|_, _| Ok(()));

    let dep1 = Deployment {
        id: dep1_id,
        app_id,
        user_id,
        status: "RUNNING".to_string(),
        job_id: Some("job-1".to_string()),
        ..Default::default()
    };

    let all_deps = vec![dep1.clone(), dep2.clone()];
    mock_app_repo
        .expect_list_deployments_by_app()
        .returning(move |_| Ok(all_deps.clone()));

    // Expect dep1 to be paused
    mock_scheduler
        .expect_pause_app()
        .with(eq("job-1".to_string()), eq(user_id.to_string()))
        .times(1)
        .returning(|_, _| Ok(true));

    // Expect dep1 status update to STOPPED
    mock_app_repo
        .expect_update_deployment()
        .with(
            eq(dep1_id),
            predicate::function(|params: &UpdateDeploymentParams| {
                params.status == Some("STOPPED".to_string())
                    && params.job_id == Some("job-1".to_string())
            }),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    // Expect dep2 to be resumed
    mock_scheduler
        .expect_resume_app()
        .with(eq("job-2".to_string()), eq(user_id.to_string()))
        .times(1)
        .returning(|_, _| Ok(true));

    // Expect dep2 status update to RUNNING
    mock_app_repo
        .expect_update_deployment()
        .with(
            eq(dep2_id),
            predicate::function(|params: &UpdateDeploymentParams| {
                params.status == Some("RUNNING".to_string())
                    && params.job_id == Some("job-2".to_string())
            }),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();
    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        scheduler: Arc::new(mock_scheduler),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client),
        router_addr: "http://localhost:8080".to_string(),
        api_db: sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/dummy")
            .unwrap(),
        jwt_secret: jwt_secret.into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        acme_email: "admin@mikrom.spluca.org".into(),
        acme_staging: true,
        acme_check_interval: 3600,
    };

    let router = create_app(state);

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/apps/test-app/deployments/{}/activate", dep2_id))
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_promotion_pauses_previous_active() {
    let mock_user_repo = MockUserRepository::new();
    let mut mock_app_repo = MockAppRepository::new();
    let mut mock_scheduler = MockScheduler::new();

    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let new_dep_id = Uuid::new_v4();
    let old_dep_id = Uuid::new_v4();
    let jwt_secret = "test-secret";

    let token = mikrom_api::auth::jwt::create_token(
        &user_id.to_string(),
        "test@example.com",
        &mikrom_api::repositories::user_repository::UserRole::User,
        jwt_secret,
    )
    .unwrap();

    // 1. Mock get_app_by_name
    let app = App {
        id: app_id,
        name: "test-app".to_string(),
        git_url: "git".to_string(),
        port: 8080,
        hostname: None,
        user_id,
        github_webhook_secret: None,
        active_deployment_id: Some(old_dep_id),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let app_for_mock = app.clone();
    mock_app_repo
        .expect_get_app_by_name()
        .returning(move |_| Ok(Some(app_for_mock.clone())));

    // 2. Mock get_deployment for the new deployment
    let new_dep = Deployment {
        id: new_dep_id,
        app_id,
        user_id,
        status: "RUNNING".to_string(),
        job_id: Some("job-new".to_string()),
        ..Default::default()
    };
    let new_dep_clone = new_dep.clone();
    mock_app_repo
        .expect_get_deployment()
        .with(eq(new_dep_id))
        .returning(move |_| Ok(Some(new_dep_clone.clone())));

    // Mock get_app
    let app_clone = app.clone();
    mock_app_repo
        .expect_get_app()
        .with(eq(app_id))
        .returning(move |_| Ok(Some(app_clone.clone())));

    // 3. Mock set_active_deployment
    mock_app_repo
        .expect_set_active_deployment()
        .with(eq(app_id), eq(new_dep_id))
        .returning(|_, _| Ok(()));

    // 4. Mock list_deployments_by_app to include the old deployment
    let old_dep = Deployment {
        id: old_dep_id,
        app_id,
        user_id,
        status: "RUNNING".to_string(),
        job_id: Some("job-old".to_string()),
        ..Default::default()
    };
    let all_deps = vec![new_dep.clone(), old_dep.clone()];
    mock_app_repo
        .expect_list_deployments_by_app()
        .returning(move |_| Ok(all_deps.clone()));

    // Expect hibernation of old_dep
    mock_scheduler
        .expect_pause_app()
        .with(eq("job-old".to_string()), eq(user_id.to_string()))
        .times(1)
        .returning(|_, _| Ok(true));

    // Expect resume of new_dep
    mock_scheduler
        .expect_resume_app()
        .with(eq("job-new".to_string()), eq(user_id.to_string()))
        .times(1)
        .returning(|_, _| Ok(true));

    // 5. Mock update_deployment for the old deployment (marking it STOPPED)
    mock_app_repo
        .expect_update_deployment()
        .with(
            eq(old_dep_id),
            predicate::function(|params: &UpdateDeploymentParams| {
                params.status == Some("STOPPED".to_string())
            }),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    // Expect update for the new deployment (marking it RUNNING)
    mock_app_repo
        .expect_update_deployment()
        .with(
            eq(new_dep_id),
            predicate::function(|params: &UpdateDeploymentParams| {
                params.status == Some("RUNNING".to_string())
            }),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();
    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        scheduler: Arc::new(mock_scheduler),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client),
        router_addr: "http://localhost:8080".to_string(),
        api_db: sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/dummy")
            .unwrap(),
        jwt_secret: jwt_secret.into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        acme_email: "admin@mikrom.spluca.org".into(),
        acme_staging: true,
        acme_check_interval: 3600,
    };

    let router = create_app(state);

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/apps/{}/deployments/{}/activate",
                    "test-app", new_dep_id
                ))
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Give background task a moment to run
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_activate_stopped_deployment_resumes_it() {
    let mock_user_repo = MockUserRepository::new();
    let mut mock_app_repo = MockAppRepository::new();
    let mut mock_scheduler = MockScheduler::new();

    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let dep_id = Uuid::new_v4();
    let jwt_secret = "test-secret";

    let token = mikrom_api::auth::jwt::create_token(
        &user_id.to_string(),
        "test@example.com",
        &mikrom_api::repositories::user_repository::UserRole::User,
        jwt_secret,
    )
    .unwrap();

    // 1. Mock get_app_by_name
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
    let app_for_mock = app.clone();
    mock_app_repo
        .expect_get_app_by_name()
        .returning(move |_| Ok(Some(app_for_mock.clone())));

    // 2. Mock get_deployment for the PAUSED deployment
    let paused_dep = Deployment {
        id: dep_id,
        app_id,
        user_id,
        status: "STOPPED".to_string(),
        job_id: Some("job-stopped".to_string()),
        ..Default::default()
    };
    let paused_dep_clone = paused_dep.clone();
    mock_app_repo
        .expect_get_deployment()
        .with(eq(dep_id))
        .returning(move |_| Ok(Some(paused_dep_clone.clone())));

    // Mock get_app
    let app_clone = app.clone();
    mock_app_repo
        .expect_get_app()
        .with(eq(app_id))
        .returning(move |_| Ok(Some(app_clone.clone())));

    // 3. Mock set_active_deployment
    mock_app_repo
        .expect_set_active_deployment()
        .returning(|_, _| Ok(()));

    // 4. Mock list_deployments_by_app
    let paused_dep_clone2 = paused_dep.clone();
    mock_app_repo
        .expect_list_deployments_by_app()
        .returning(move |_| Ok(vec![paused_dep_clone2.clone()]));

    // Expect resumption
    mock_scheduler
        .expect_resume_app()
        .with(eq("job-stopped".to_string()), eq(user_id.to_string()))
        .times(1)
        .returning(|_, _| Ok(true));

    // 5. Mock update_deployment for resuming (marking it RUNNING)
    mock_app_repo
        .expect_update_deployment()
        .with(
            eq(dep_id),
            predicate::function(|params: &UpdateDeploymentParams| {
                params.status == Some("RUNNING".to_string())
                    && params.job_id == Some("job-stopped".to_string())
            }),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();
    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        scheduler: Arc::new(mock_scheduler),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client),
        router_addr: "http://localhost:8080".to_string(),
        api_db: sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/dummy")
            .unwrap(),
        jwt_secret: jwt_secret.into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        acme_email: "admin@mikrom.spluca.org".into(),
        acme_staging: true,
        acme_check_interval: 3600,
    };

    let router = create_app(state);

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/apps/{}/deployments/{}/activate",
                    "test-app", dep_id
                ))
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Give background task a moment to run
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_delete_app_cleans_up_resources() {
    let mock_user_repo = MockUserRepository::new();
    let mut mock_app_repo = MockAppRepository::new();
    let mut mock_scheduler = MockScheduler::new();

    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let dep_id = Uuid::new_v4();
    let jwt_secret = "test-secret";

    let token = mikrom_api::auth::jwt::create_token(
        &user_id.to_string(),
        "test@example.com",
        &mikrom_api::repositories::user_repository::UserRole::User,
        jwt_secret,
    )
    .unwrap();

    // 1. Mock get_app_by_name
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
    let app_for_mock = app.clone();
    mock_app_repo
        .expect_get_app_by_name()
        .returning(move |_| Ok(Some(app_for_mock.clone())));

    // 2. Mock list_deployments_by_app to return one deployment with job_id
    let dep = Deployment {
        id: dep_id,
        app_id,
        user_id,
        status: "RUNNING".to_string(),
        job_id: Some("job-to-delete".to_string()),
        ..Default::default()
    };
    mock_app_repo
        .expect_list_deployments_by_app()
        .with(eq(app_id))
        .returning(move |_| Ok(vec![dep.clone()]));

    // Expect deletion in scheduler
    mock_scheduler
        .expect_delete_all_by_app()
        .with(eq(app_id.to_string()), eq(user_id.to_string()))
        .times(1)
        .returning(|_, _| Ok(true));

    // 3. Mock delete_app
    mock_app_repo
        .expect_delete_app()
        .with(eq(app_id))
        .times(1)
        .returning(|_| Ok(()));

    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();
    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        scheduler: Arc::new(mock_scheduler),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client),
        router_addr: "http://localhost:8080".to_string(),
        api_db: sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/dummy")
            .unwrap(),
        jwt_secret: jwt_secret.into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        acme_email: "admin@mikrom.spluca.org".into(),
        acme_staging: true,
        acme_check_interval: 3600,
    };

    let router = create_app(state);

    let response = router
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/apps/test-app")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}
