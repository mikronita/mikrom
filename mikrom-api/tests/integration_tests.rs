use std::env;
use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use sqlx::PgPool;
use tower::ServiceExt;

use mikrom_api::AppState;
use mikrom_api::auth::{login, register};
use mikrom_api::repositories::postgres_user_repository::PostgresUserRepository;

async fn setup_test_pool() -> PgPool {
    let connection_string = env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://mikrom:mikrom_password@localhost:5432/mikrom_api".to_string()
    });

    let pool = PgPool::connect(&connection_string)
        .await
        .expect("Failed to connect to test db");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    pool
}

fn create_app(pool: PgPool, jwt_secret: &str) -> axum::Router {
    let user_repo = PostgresUserRepository::new(Arc::new(pool));
    let state = AppState {
        user_repo: Arc::new(user_repo),
        scheduler_client: None,
        scheduler_config: mikrom_api::scheduler::SchedulerConfig::default(),
        jwt_secret: jwt_secret.to_string(),
        master_key: "integration-master-key".into(),
    };
    axum::Router::new()
        .route("/auth/register", axum::routing::post(register))
        .route("/auth/login", axum::routing::post(login))
        .with_state(state)
}

#[tokio::test]
async fn test_register_full_flow() {
    let pool = setup_test_pool().await;
    let app = create_app(pool, "integration-test-secret");
    let email = format!("full_flow_{}@example.com", uuid::Uuid::new_v4());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/register")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "email": email,
                        "password": "password123"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);

    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["message"], "User registered successfully");
    let user_id = json["user_id"].as_str().unwrap();
    assert!(!user_id.is_empty());
}

#[tokio::test]
async fn test_login_full_flow() {
    let pool = setup_test_pool().await;
    let app = create_app(pool, "integration-test-secret");
    let email = format!("login_full_{}@example.com", uuid::Uuid::new_v4());

    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/register")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "email": email,
                        "password": "mypassword123"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/login")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "email": email,
                        "password": "mypassword123"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let token = json["token"].as_str().unwrap();
    assert!(token.starts_with("eyJ"));
}

#[tokio::test]
async fn test_password_hash_long_password() {
    let pool = setup_test_pool().await;
    let app = create_app(pool, "integration-test-secret");
    let email = format!("hash_long_{}@example.com", uuid::Uuid::new_v4());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/register")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "email": email,
                        "password": "a".repeat(1000)
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn test_multiple_registrations() {
    let pool = setup_test_pool().await;
    let app = create_app(pool, "integration-test-secret");
    let email = format!("multi_{}@example.com", uuid::Uuid::new_v4());

    for i in 0..3 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/register")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "email": &email,
                            "password": format!("password{}", i)
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        if i == 0 {
            assert_eq!(response.status(), StatusCode::CREATED);
        } else {
            assert_eq!(response.status(), StatusCode::CONFLICT);
        }
    }
}

#[tokio::test]
async fn test_register_and_login_workflow() {
    let pool = setup_test_pool().await;
    let app = create_app(pool, "integration-test-secret");
    let email = format!("workflow_{}@example.com", uuid::Uuid::new_v4());
    let password = "securePassword123";

    let register_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/register")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "email": email,
                        "password": password
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(register_response.status(), StatusCode::CREATED);

    let login_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/login")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "email": email,
                        "password": password
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(login_response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(login_response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let token = json["token"].as_str().unwrap();

    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 3);
}

#[tokio::test]
async fn test_login_token_creation_with_valid_secret() {
    let pool = setup_test_pool().await;
    let app = create_app(pool, "integration-test-secret");
    let email = format!("token_valid_{}@example.com", uuid::Uuid::new_v4());

    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/register")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "email": email,
                        "password": "password123"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/login")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "email": email,
                        "password": "password123"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["token"].as_str().is_some());
}
