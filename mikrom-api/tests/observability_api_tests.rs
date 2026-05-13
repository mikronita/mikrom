use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mikrom_api::AppState;
use mikrom_api::auth::jwt::create_token;
use mikrom_api::create_app;
use mikrom_api::repositories::app_repository::MockAppRepository;
use mikrom_api::repositories::user_repository::{MockUserRepository, UserRole};
use std::sync::Arc;
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
        github_repo: Arc::new(mikrom_api::repositories::MockGithubRepository::default()),
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
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    create_app(state)
}

#[tokio::test]
async fn test_app_logs_stream_auth() {
    let mut mock_app_repo = MockAppRepository::new();
    let app_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    let app_name = "test-logs-app";

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
        .uri(format!("/v1/apps/{}/logs/stream", app_name))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.call(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
}

#[tokio::test]
async fn test_app_metrics_stream_auth() {
    let mut mock_app_repo = MockAppRepository::new();
    let app_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    let app_name = "test-metrics-app";

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
        .uri(format!("/v1/apps/{}/metrics/stream", app_name))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.call(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
}
