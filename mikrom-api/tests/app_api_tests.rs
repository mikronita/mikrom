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
use mikrom_api::repositories::{MockAppRepository, MockUserRepository};

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
        .with(
            eq(app_name),
            eq(git_url),
            eq(8080),
            eq(Some("test-app.apps.mikrom.es".to_string())),
            eq(user_id.to_string()),
            always(),
        )
        .times(1)
        .returning(move |name, url, port, hostname, uid, secret| {
            Ok(App {
                id: app_id,
                name: name.to_string(),
                git_url: url.to_string(),
                port,
                hostname: hostname.map(|s| s.to_string()),
                user_id: Uuid::parse_str(uid).unwrap(),
                github_webhook_secret: secret,
                active_deployment_id: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
        });

    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
        scheduler_config: Default::default(),
        builder_addr: "http://localhost:5004".into(),
        router_addr: "http://localhost:8080".to_string(),
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
                .uri("/apps")
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

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let app_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(app_resp["name"], app_name);
    assert_eq!(app_resp["git_url"], git_url);
    assert_eq!(app_resp["port"], 8080);
    assert_eq!(app_resp["hostname"], "test-app.apps.mikrom.es");
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
        .returning(move |name, _, _, _, _, _| {
            Err(anyhow::anyhow!(
                "Application name '{}' is already taken",
                name
            ))
        });

    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
        scheduler_config: Default::default(),
        builder_addr: "http://localhost:5004".into(),
        router_addr: "http://localhost:8080".to_string(),
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
                .uri("/apps")
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
