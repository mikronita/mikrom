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

async fn setup_test_context() -> (String, Uuid, Uuid, MockAppRepository) {
    let user_id = Uuid::new_v4();
    let jwt_secret = "test-secret";

    let token = mikrom_api::auth::jwt::create_token(
        &user_id.to_string(),
        "test@example.com",
        &mikrom_api::repositories::user_repository::UserRole::User,
        jwt_secret,
    )
    .unwrap();

    (
        token,
        user_id,
        Uuid::new_v4(), // Placeholder for app_id
        MockAppRepository::new(),
    )
}

#[tokio::test]
async fn test_hierarchical_deployment_status_success() {
    let (token, user_id, app_id, mut mock_app_repo) = setup_test_context().await;
    let app_name = "test-app";
    // Using a temp- ID bypasses the NATS call to the scheduler in the handler
    let job_id = "temp-66504281-4065-4f43-9f6e-b9146647f084";
    let dep_id = Uuid::parse_str("66504281-4065-4f43-9f6e-b9146647f084").unwrap();

    // 1. Mock get_app_by_name
    mock_app_repo
        .expect_get_app_by_name()
        .with(eq(app_name))
        .returning(move |_| {
            Ok(Some(App {
                id: app_id,
                name: app_name.to_string(),
                git_url: "git".to_string(),
                port: 8080,
                user_id,
                ..Default::default()
            }))
        });

    // 2. Mock get_deployment
    mock_app_repo
        .expect_get_deployment()
        .with(eq(dep_id))
        .returning(move |_| {
            Ok(Some(Deployment {
                id: dep_id,
                app_id,
                user_id,
                job_id: None,
                status: "BUILDING".to_string(),
                image_tag: Some("nginx".to_string()),
                vcpus: 1,
                memory_mib: 256,
                disk_mib: 1024,
                port: 8080,
                env_vars: serde_json::json!({}),
                build_id: None,
                ipv6_address: None,
                trigger_source: "manual".to_string(),
                git_commit_hash: None,
                git_commit_message: None,
                git_branch: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            }))
        });

    let mock_scheduler = mikrom_api::scheduler::MockScheduler::new();

    // We still need a NATS client to satisfy AppState, but it won't be used
    // because we are using a temp- ID.
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();
    let state = AppState {
        user_repo: Arc::new(MockUserRepository::new()),
        app_repo: Arc::new(mock_app_repo),
        scheduler: Arc::new(mock_scheduler),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        api_db: sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/dummy")
            .unwrap(),
        jwt_secret: "test-secret".into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        acme_email: "admin@mikrom.spluca.org".into(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_repo: Arc::new(mikrom_api::repositories::MockGithubRepository::default()),
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        mesh_status: tokio::sync::watch::channel(mikrom_api::vms::MeshStatus::default()).0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    let router = create_app(state);

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/apps/{}/deployments/{}", app_name, job_id))
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_hierarchical_security_cross_app_prevention() {
    let (token, user_id, app_a_id, mut mock_app_repo) = setup_test_context().await;
    let app_a_name = "app-a";
    let app_b_id = Uuid::new_v4();
    let job_id = "temp-66504281-4065-4f43-9f6e-b9146647f084";
    let dep_id = Uuid::parse_str("66504281-4065-4f43-9f6e-b9146647f084").unwrap();

    mock_app_repo
        .expect_get_app_by_name()
        .with(eq(app_a_name))
        .returning(move |_| {
            Ok(Some(App {
                id: app_a_id,
                name: app_a_name.to_string(),
                git_url: "git".to_string(),
                port: 8080,
                user_id,
                ..Default::default()
            }))
        });

    // For temp- IDs, it calls get_deployment(dep_id)
    mock_app_repo
        .expect_get_deployment()
        .with(eq(dep_id))
        .returning(move |_| {
            Ok(Some(Deployment {
                id: dep_id,
                app_id: app_b_id, // Belongs to App B!
                user_id,
                job_id: None,
                status: "BUILDING".to_string(),
                image_tag: Some("nginx".to_string()),
                vcpus: 1,
                memory_mib: 256,
                disk_mib: 1024,
                port: 8080,
                env_vars: serde_json::json!({}),
                build_id: None,
                ipv6_address: None,
                trigger_source: "manual".to_string(),
                git_commit_hash: None,
                git_commit_message: None,
                git_branch: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            }))
        });

    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();
    let state = AppState {
        user_repo: Arc::new(MockUserRepository::new()),
        app_repo: Arc::new(mock_app_repo),
        scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        api_db: sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/dummy")
            .unwrap(),
        jwt_secret: "test-secret".into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        acme_email: "admin@mikrom.spluca.org".into(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_repo: Arc::new(mikrom_api::repositories::MockGithubRepository::default()),
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        mesh_status: tokio::sync::watch::channel(mikrom_api::vms::MeshStatus::default()).0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    let router = create_app(state);

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/apps/{}/deployments/{}", app_a_name, job_id))
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
