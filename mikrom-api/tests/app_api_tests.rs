use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mockall::predicate::*;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

use chrono::Utc;
use mikrom_api::AppState;
use mikrom_api::create_app;
use mikrom_api::models::app::App;
use mikrom_api::repositories::{
    MockAppRepository, MockUserRepository, app_repository::CreateAppParams,
};
use mikrom_api::test_utils::TestDb;

#[tokio::test]
async fn test_create_app_endpoint() {
    let mock_user_repo = MockUserRepository::new();
    let mut mock_app_repo = MockAppRepository::new();

    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();

    let app_name = "test-app";
    let git_url = "https://github.com/test/repo";
    let jwt_secret = "test-secret";

    // Create a real token for the test
    let token = mikrom_api::auth::jwt::create_token(
        &user_id.to_string(),
        "test@example.com",
        &mikrom_api::repositories::user_repository::UserRole::User,
        jwt_secret,
    )
    .unwrap();

    // Expectation: repo should be called with specific params
    mock_app_repo
        .expect_create_app()
        .with(mockall::predicate::function(move |p: &CreateAppParams| {
            p.name == "test-app" && p.git_url == "https://github.com/test/repo"
        }))
        .times(1)
        .returning(move |params| {
            Ok(App {
                id: app_id,
                name: params.name,
                git_url: params.git_url,
                port: params.port,
                hostname: params.hostname,
                user_id: params.user_id,
                github_webhook_secret: params.github_webhook_secret,
                github_installation_id: params.github_installation_id,
                github_repo_id: params.github_repo_id,
                github_repo_full_name: params.github_repo_full_name,
                active_deployment_id: None,
                health_check_path: params.health_check_path.unwrap_or_else(|| "/".to_string()),
                drain_timeout: params.drain_timeout.unwrap_or(10),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
        });

    let db = TestDb::new().await;
    let db_pool = db.pool().clone();

    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();
    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        volume_repo: Arc::new(
            mikrom_api::repositories::volume_repository::MockVolumeRepository::new(),
        ),
        github_repo: Arc::new(mikrom_api::repositories::MockGithubRepository::default()),
        scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
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
                .method("POST")
                .uri("/v1/apps")
                .header("Content-Type", "application/json")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::from(format!(
                    r#"{{"name": "{}", "git_url": "{}"}}"#,
                    app_name, git_url
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);

    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let app_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(app_resp["name"], app_name);
    assert_eq!(app_resp["git_url"], git_url);
    assert_eq!(app_resp["port"], 8080);
    assert!(app_resp["github_webhook_secret"].is_string());
    assert!(
        !app_resp["github_webhook_secret"]
            .as_str()
            .unwrap()
            .is_empty()
    );
    assert_eq!(app_resp["hostname"], "test-app.apps.mikrom.spluca.org");
}

#[tokio::test]
async fn test_create_app_duplicate_name() {
    let mock_user_repo = mikrom_api::repositories::user_repository::MockUserRepository::new();
    let mut mock_app_repo = MockAppRepository::new();

    let user_id = Uuid::new_v4();
    let app_name = "already-exists";
    let jwt_secret = "test-secret";

    let token = mikrom_api::auth::jwt::create_token(
        &user_id.to_string(),
        "test@example.com",
        &mikrom_api::repositories::user_repository::UserRole::User,
        jwt_secret,
    )
    .unwrap();

    // Mock: repo returns error for duplicate name
    mock_app_repo
        .expect_create_app()
        .times(1)
        .returning(move |params| {
            Err(anyhow::anyhow!(
                "Application name '{}' is already taken",
                params.name
            ))
        });

    let db = TestDb::new().await;
    let db_pool = db.pool().clone();

    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();
    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        volume_repo: Arc::new(
            mikrom_api::repositories::volume_repository::MockVolumeRepository::new(),
        ),
        github_repo: Arc::new(mikrom_api::repositories::MockGithubRepository::default()),
        scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
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
                .method("POST")
                .uri("/v1/apps")
                .header("Content-Type", "application/json")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::from(format!(
                    r#"{{"name": "{}", "git_url": "git"}}"#,
                    app_name
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);

    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let error_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        error_resp["error"]
            .as_str()
            .unwrap()
            .contains("already taken")
    );
}

#[tokio::test]
async fn test_list_apps_includes_secret() {
    let mock_user_repo = MockUserRepository::new();
    let mut mock_app_repo = MockAppRepository::new();

    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let jwt_secret = "test-secret";

    let token = mikrom_api::auth::jwt::create_token(
        &user_id.to_string(),
        "test@example.com",
        &mikrom_api::repositories::user_repository::UserRole::User,
        jwt_secret,
    )
    .unwrap();

    let secret = "test-webhook-secret-123".to_string();

    mock_app_repo
        .expect_list_apps_by_user()
        .with(eq(Some(user_id)))
        .times(1)
        .returning(move |_| {
            Ok(vec![App {
                id: app_id,
                name: "test-app".to_string(),
                git_url: "git".to_string(),
                port: 8080,
                hostname: Some("test-app.apps.mikrom.spluca.org".to_string()),
                user_id,
                github_webhook_secret: Some(secret.clone()),
                github_installation_id: None,
                github_repo_id: None,
                github_repo_full_name: None,
                active_deployment_id: None,
                health_check_path: "/".to_string(),
                drain_timeout: 10,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            }])
        });

    let db = TestDb::new().await;
    let db_pool = db.pool().clone();

    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();
    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        volume_repo: Arc::new(
            mikrom_api::repositories::volume_repository::MockVolumeRepository::new(),
        ),
        github_repo: Arc::new(mikrom_api::repositories::MockGithubRepository::default()),
        scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
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
                .uri("/v1/apps")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let apps_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(apps_resp.is_array());
    let app = &apps_resp[0];
    assert_eq!(app["github_webhook_secret"], "********");
    assert_eq!(app["hostname"], "test-app.apps.mikrom.spluca.org");
}

#[tokio::test]
async fn test_get_app_secret_endpoint() {
    let mock_user_repo = MockUserRepository::new();
    let mut mock_app_repo = MockAppRepository::new();

    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let app_name = "secret-app";
    let jwt_secret = "test-secret";
    let webhook_secret = "real-secret-123";

    let token = mikrom_api::auth::jwt::create_token(
        &user_id.to_string(),
        "test@example.com",
        &mikrom_api::repositories::user_repository::UserRole::User,
        jwt_secret,
    )
    .unwrap();

    mock_app_repo
        .expect_get_app_by_name()
        .with(eq(app_name))
        .times(1)
        .returning(move |name| {
            Ok(Some(App {
                id: app_id,
                name: name.to_string(),
                git_url: "git".to_string(),
                port: 8080,
                hostname: None,
                user_id,
                github_webhook_secret: Some(webhook_secret.to_string()),
                github_installation_id: None,
                github_repo_id: None,
                github_repo_full_name: None,
                active_deployment_id: None,
                health_check_path: "/".to_string(),
                drain_timeout: 10,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            }))
        });

    let db = TestDb::new().await;
    let db_pool = db.pool().clone();

    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();
    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        volume_repo: Arc::new(
            mikrom_api::repositories::volume_repository::MockVolumeRepository::new(),
        ),
        github_repo: Arc::new(mikrom_api::repositories::MockGithubRepository::default()),
        scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
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
                .uri(format!("/v1/apps/{}/secret", app_name))
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let secret_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(secret_resp["github_webhook_secret"], "real-secret-123");
}

#[tokio::test]
async fn test_create_app_with_custom_config() {
    use axum::Json;
    use axum::extract::State;
    use mikrom_api::auth::AuthUser;
    use mikrom_api::deploy::handlers::{CreateAppRequest, create_app_handler};
    use mikrom_api::repositories::{MockGithubRepository, MockUserRepository};
    use mikrom_api::scheduler::MockScheduler;
    use mockall::predicate;

    let mut mock_app_repo = MockAppRepository::new();
    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();

    let request = CreateAppRequest {
        name: "custom-app".to_string(),
        git_url: "https://github.com/custom/repo".to_string(),
        port: Some(3000),
        github_installation_id: None,
        github_repo_id: None,
        github_repo_full_name: None,
        health_check_path: Some("/healthz".to_string()),
        drain_timeout: Some(60),
    };

    mock_app_repo
        .expect_create_app()
        .with(predicate::function(
            |params: &mikrom_api::repositories::app_repository::CreateAppParams| {
                params.name == "custom-app"
                    && params.health_check_path == Some("/healthz".to_string())
                    && params.drain_timeout == Some(60)
            },
        ))
        .times(1)
        .returning(move |params| {
            Ok(App {
                id: app_id,
                name: params.name,
                git_url: params.git_url,
                port: params.port,
                user_id: params.user_id,
                health_check_path: params.health_check_path.unwrap(),
                drain_timeout: params.drain_timeout.unwrap(),
                ..Default::default()
            })
        });

    let state = AppState {
        user_repo: Arc::new(MockUserRepository::new()),
        app_repo: Arc::new(mock_app_repo),
        volume_repo: Arc::new(
            mikrom_api::repositories::volume_repository::MockVolumeRepository::new(),
        ),
        github_repo: Arc::new(MockGithubRepository::default()),
        scheduler: Arc::new(MockScheduler::new()),
        nats: mikrom_api::nats::TypedNatsClient::new(
            async_nats::connect("nats://localhost:4223").await.unwrap(),
        ),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        api_db: sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap(),
        jwt_secret: "secret".to_string(),
        master_key: "key".to_string(),
        deployment_events: tokio::sync::broadcast::channel(100).0,
        acme_email: "test@example.com".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        mesh_status: tokio::sync::watch::channel(mikrom_api::vms::MeshStatus::default()).0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    let auth = AuthUser {
        user_id: user_id.to_string(),
        email: "test@example.com".to_string(),
        role: mikrom_api::repositories::user_repository::UserRole::User,
    };

    let result = create_app_handler(auth, State(state), Json(request)).await;

    let (_, Json(response)) = result.unwrap();
    assert_eq!(response.name, "custom-app");
    assert_eq!(response.health_check_path, "/healthz");
    assert_eq!(response.drain_timeout, 60);
}
