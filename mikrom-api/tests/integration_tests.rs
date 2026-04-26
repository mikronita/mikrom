use std::env;
use std::sync::Arc;

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

async fn get_test_pool() -> PgPool {
    let connection_string = env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://mikrom:mikrom_password@localhost:5432/mikrom_api".to_string()
    });

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(&connection_string)
        .await
        .expect("Failed to connect to test db");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    pool
}

fn create_app(pool: PgPool, jwt_secret: &str) -> axum::Router {
    let user_repo = PostgresUserRepository::new(pool.clone());
    let app_repo = PostgresAppRepository::new(pool.clone());
    let state = AppState {
        user_repo: Arc::new(user_repo),
        app_repo: Arc::new(app_repo),
        scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
        scheduler_config: mikrom_api::scheduler::SchedulerConfig::default(),
        builder_addr: "http://localhost:5004".to_string(),
        router_addr: "http://localhost:8080".to_string(),
        jwt_secret: jwt_secret.to_string(),
        master_key: "integration-master-key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        build_semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
    };
    axum::Router::new()
        .route("/auth/register", axum::routing::post(register))
        .route("/auth/login", axum::routing::post(login))
        .route("/auth/me", axum::routing::get(get_profile))
        .route("/auth/me", axum::routing::put(update_profile))
        .route(
            "/apps",
            axum::routing::post(mikrom_api::deploy::create_app_handler),
        )
        .route(
            "/apps",
            axum::routing::get(mikrom_api::deploy::list_apps_handler),
        )
        .route(
            "/apps/:app_id",
            axum::routing::delete(mikrom_api::deploy::delete_app_handler),
        )
        .with_state(state)
}

#[tokio::test]
async fn test_all_auth_integration_flows() {
    let pool = get_test_pool().await;
    let jwt_secret = "integration-test-secret";

    // Test 1: Register and Login Workflow
    {
        let app = create_app(pool.clone(), jwt_secret);
        let email = format!("workflow_{}@example.com", uuid::Uuid::new_v4());
        let password = "securePassword123";

        let reg_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/register")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"email": email, "password": password}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(reg_resp.status(), StatusCode::CREATED);

        let login_resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/login")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"email": email, "password": password}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(login_resp.status(), StatusCode::OK);
    }

    // Test 2: Profile Flow
    {
        let app = create_app(pool.clone(), jwt_secret);
        let email = format!("profile_{}@example.com", uuid::Uuid::new_v4());
        let password = "password123";

        // Register
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/register")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"email": email, "password": password}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Login
        let login_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/login")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"email": email, "password": password}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(login_resp.into_body(), 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let token = json["token"].as_str().unwrap();

        // Get Profile
        let get_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/auth/me")
                    .header("Authorization", format!("Bearer {}", token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_resp.status(), StatusCode::OK);

        // Update Profile
        let up_resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/auth/me")
                    .header("Content-Type", "application/json")
                    .header("Authorization", format!("Bearer {}", token))
                    .body(Body::from(
                        serde_json::json!({"first_name": "Test", "last_name": "User"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(up_resp.status(), StatusCode::OK);
    }

    // Test 3: Multiple Registrations (Conflict)
    {
        let app = create_app(pool.clone(), jwt_secret);
        let email = format!("multi_{}@example.com", uuid::Uuid::new_v4());

        for i in 0..2 {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/auth/register")
                        .header("Content-Type", "application/json")
                        .body(Body::from(
                            serde_json::json!({"email": email, "password": "password123"})
                                .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();

            if i == 0 {
                assert_eq!(resp.status(), StatusCode::CREATED);
            } else {
                assert_eq!(resp.status(), StatusCode::CONFLICT);
            }
        }
    }

    // Test 4: Password Hashing (Long Password)
    {
        let app = create_app(pool.clone(), jwt_secret);
        let email = format!("hash_long_{}@example.com", uuid::Uuid::new_v4());

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/register")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"email": email, "password": "a".repeat(100)})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    // Test 5: Login Full Flow
    {
        let app = create_app(pool.clone(), jwt_secret);
        let email = format!("login_full_{}@example.com", uuid::Uuid::new_v4());
        let password = "password123";

        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/register")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"email": email, "password": password}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let login_resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/login")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"email": email, "password": password}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(login_resp.status(), StatusCode::OK);
    }

    // Test 6: App Lifecycle
    {
        let app = create_app(pool.clone(), jwt_secret);
        let email = format!("app_life_{}@example.com", uuid::Uuid::new_v4());
        let password = "password123";

        // 1. Register & Login to get token
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/register")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"email": email, "password": password}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let login_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/login")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"email": email, "password": password}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(login_resp.into_body(), 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let token = json["token"].as_str().unwrap();

        // 2. Create App (requires auth)
        let app_name = format!("integration-app-{}", uuid::Uuid::new_v4());
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/apps")
                    .header("Authorization", format!("Bearer {}", token))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": app_name,
                            "git_url": "https://github.com/mikrom/test"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::OK);

        // 3. List Apps
        let list_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/apps")
                    .header("Authorization", format!("Bearer {}", token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list_resp.status(), StatusCode::OK);

        // 4. Delete App
        let del_resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/apps/{}", app_name))
                    .header("Authorization", format!("Bearer {}", token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);
    }
}
