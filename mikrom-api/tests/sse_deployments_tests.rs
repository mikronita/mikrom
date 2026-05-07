use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mikrom_api::AppState;
use mikrom_api::auth::jwt::create_token;
use mikrom_api::create_app;
use mikrom_api::models::app::Deployment;
use mikrom_api::repositories::app_repository::MockAppRepository;
use mikrom_api::repositories::user_repository::{MockUserRepository, UserRole};
use std::sync::Arc;
use tokio_stream::StreamExt;
use tower::Service;
use uuid::Uuid;

const JWT_SECRET: &str = "test-secret";

async fn setup_app(mock_app_repo: MockAppRepository) -> axum::Router {
    let mock_user_repo = MockUserRepository::new();
    let (deployment_events, _) = tokio::sync::broadcast::channel(100);
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();

    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        api_db: sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/dummy")
            .unwrap(),
        jwt_secret: JWT_SECRET.into(),
        master_key: "key".into(),
        deployment_events: deployment_events.clone(),
        acme_email: "admin@mikrom.spluca.org".into(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_repo: Arc::new(mikrom_api::repositories::MockGithubRepository::default()),
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    create_app(state)
}

#[tokio::test]
async fn test_sse_deployments_stream_initial_data() {
    let mut mock_app_repo = MockAppRepository::new();
    let app_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    let app_name = "test-app";

    // Mock app ownership check
    let app_id_clone = app_id;
    mock_app_repo
        .expect_get_app_by_name()
        .returning(move |name| {
            if name == "test-app" {
                Ok(Some(mikrom_api::models::app::App {
                    id: app_id,
                    name: "test-app".to_string(),
                    git_url: "git".to_string(),
                    port: 8080,
                    user_id,
                    ..Default::default()
                }))
            } else {
                Ok(None)
            }
        });

    // Mock initial deployments
    mock_app_repo
        .expect_list_deployments_by_app()
        .returning(move |_| {
            Ok(vec![Deployment {
                id: Uuid::new_v4(),
                app_id: app_id_clone,
                user_id,
                status: "RUNNING".into(),
                job_id: Some("job-1".into()),
                image_tag: Some("nginx:latest".into()),
                build_id: None,
                port: 80,
                vcpus: 1,
                memory_mib: 256,
                disk_mib: 1024,
                env_vars: serde_json::json!({}),
                git_commit_hash: None,
                git_commit_message: None,
                git_branch: None,
                trigger_source: "manual".into(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                ip_address: None,
            }])
        });

    let mut router = setup_app(mock_app_repo).await;
    let token = create_token(
        &user_id.to_string(),
        "test@test.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/apps/{}/deployments/stream", app_name))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.call(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");

    let mut body_stream = response.into_body().into_data_stream();
    let chunk = body_stream.next().await.unwrap().unwrap();
    let chunk_str = String::from_utf8_lossy(&chunk);

    assert!(chunk_str.contains("data:"));
    assert!(chunk_str.contains("job-1"));
    assert!(chunk_str.contains("RUNNING"));
}

#[tokio::test]
async fn test_sse_deployments_auth_via_query_param() {
    let mut mock_app_repo = MockAppRepository::new();
    let app_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    let app_name = "test-app";

    mock_app_repo.expect_get_app_by_name().returning(move |_| {
        Ok(Some(mikrom_api::models::app::App {
            id: app_id,
            name: "test-app".to_string(),
            git_url: "git".to_string(),
            port: 8080,
            user_id,
            ..Default::default()
        }))
    });

    mock_app_repo
        .expect_list_deployments_by_app()
        .returning(|_| Ok(vec![]));

    let mut router = setup_app(mock_app_repo).await;
    let token = create_token(
        &user_id.to_string(),
        "test@test.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    // No Authorization header, but token in query param
    let req = Request::builder()
        .method("GET")
        .uri(format!(
            "/v1/apps/{}/deployments/stream?token={}",
            app_name, token
        ))
        .body(Body::empty())
        .unwrap();

    let response = router.call(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_sse_deployments_stream_updates() {
    let mut mock_app_repo = MockAppRepository::new();
    let app_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    let app_name = "test-app";

    mock_app_repo.expect_get_app_by_name().returning(move |_| {
        Ok(Some(mikrom_api::models::app::App {
            id: app_id,
            name: "test-app".to_string(),
            git_url: "git".to_string(),
            port: 8080,
            user_id,
            ..Default::default()
        }))
    });

    // Return empty first, then return one deployment
    let app_id_clone = app_id;
    let user_id_clone = user_id;
    mock_app_repo
        .expect_list_deployments_by_app()
        .times(1)
        .returning(|_| Ok(vec![]));

    mock_app_repo
        .expect_list_deployments_by_app()
        .times(1)
        .returning(move |_| {
            Ok(vec![Deployment {
                id: Uuid::new_v4(),
                app_id: app_id_clone,
                user_id: user_id_clone,
                status: "RUNNING".into(),
                job_id: Some("job-updated".into()),
                image_tag: Some("nginx:latest".into()),
                build_id: None,
                port: 80,
                vcpus: 1,
                memory_mib: 256,
                disk_mib: 1024,
                env_vars: serde_json::json!({}),
                git_commit_hash: None,
                git_commit_message: None,
                git_branch: None,
                trigger_source: "manual".into(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                ip_address: None,
            }])
        });

    let mock_user_repo = MockUserRepository::new();
    let (deployment_events, _) = tokio::sync::broadcast::channel(100);
    let tx = deployment_events.clone();
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();

    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        api_db: sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/dummy")
            .unwrap(),
        jwt_secret: JWT_SECRET.into(),
        master_key: "key".into(),
        deployment_events: deployment_events.clone(),
        acme_email: "admin@mikrom.spluca.org".into(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_repo: Arc::new(mikrom_api::repositories::MockGithubRepository::default()),
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    let mut router = create_app(state);
    let token = create_token(
        &user_id.to_string(),
        "test@test.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/apps/{}/deployments/stream", app_name))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.call(req).await.unwrap();
    let mut body_stream = response.into_body().into_data_stream();

    // 1. Initial empty data
    let first_chunk = body_stream.next().await.unwrap().unwrap();
    assert!(String::from_utf8_lossy(&first_chunk).contains("[]"));

    // 2. Trigger event
    tx.send(app_id).unwrap();

    // 3. Receive update
    let second_chunk = body_stream.next().await.unwrap().unwrap();
    let second_str = String::from_utf8_lossy(&second_chunk);
    assert!(second_str.contains("job-updated"));
    assert!(second_str.contains("RUNNING"));
}
