use std::env;
use std::sync::{Arc, OnceLock};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use sqlx::PgPool;
use tower::ServiceExt;

use mikrom_api::AppState;
use mikrom_api::auth::{get_profile, login, register, update_profile};
use mikrom_api::repositories::PostgresAppRepository;
use mikrom_api::repositories::postgres_user_repository::PostgresUserRepository;

static TEST_POOL: OnceLock<PgPool> = OnceLock::new();

async fn get_test_pool() -> PgPool {
    if let Some(pool) = TEST_POOL.get() {
        return pool.clone();
    }

    let connection_string = env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://mikrom:mikrom_password@localhost:5432/mikrom_api".to_string()
    });

    // Retry a few times if the DB is starting up
    let mut pool = None;
    for _ in 0..10 {
        match PgPool::connect(&connection_string).await {
            Ok(p) => {
                pool = Some(p);
                break;
            }
            Err(_) => tokio::time::sleep(std::time::Duration::from_secs(1)).await,
        }
    }

    let pool = pool.expect("Failed to connect to test db after retries");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    let _ = TEST_POOL.set(pool.clone());
    pool
}

fn create_app(pool: PgPool, jwt_secret: &str) -> axum::Router {
    let db_pool = Arc::new(pool);
    let user_repo = PostgresUserRepository::new(db_pool.clone());
    let app_repo = PostgresAppRepository::new(db_pool.clone());
    let state = AppState {
        user_repo: Arc::new(user_repo),
        app_repo: Arc::new(app_repo),
        scheduler_client: None,
        scheduler_config: mikrom_api::scheduler::SchedulerConfig::default(),
        builder_addr: "http://localhost:5004".to_string(),
        jwt_secret: jwt_secret.to_string(),
        master_key: "integration-master-key".into(),
    };
    axum::Router::new()
        .route("/auth/register", axum::routing::post(register))
        .route("/auth/login", axum::routing::post(login))
        .route("/auth/me", axum::routing::get(get_profile))
        .route("/auth/me", axum::routing::put(update_profile))
        .with_state(state)
}

#[tokio::test]
async fn test_profile_flow() {
    let pool = get_test_pool().await;
    let jwt_secret = "profile-integration-test-secret";
    let app = create_app(pool, jwt_secret);
    let email = format!("profile_flow_{}@example.com", uuid::Uuid::new_v4());
    let password = "password123";

    // 1. Register
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
                        "password": password
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // 2. Login to get token
    let response = app
        .clone()
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

    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let token = json["token"].as_str().unwrap();

    // 3. Get profile (should have null names)
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/auth/me")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["email"], email);
    assert!(json["first_name"].is_null());
    assert!(json["last_name"].is_null());

    // 4. Update profile
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/auth/me")
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "first_name": "Antonio",
                        "last_name": "Pardo"
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
    assert_eq!(json["first_name"], "Antonio");
    assert_eq!(json["last_name"], "Pardo");

    // 5. Get profile again to verify persistence
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/auth/me")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["first_name"], "Antonio");
    assert_eq!(json["last_name"], "Pardo");
}

#[tokio::test]
async fn test_register_full_flow() {
    let pool = get_test_pool().await;
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
    let pool = get_test_pool().await;
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
    let pool = get_test_pool().await;
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
    let pool = get_test_pool().await;
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
    let pool = get_test_pool().await;
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
    let pool = get_test_pool().await;
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
