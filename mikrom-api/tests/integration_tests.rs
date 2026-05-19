use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use sqlx::PgPool;
use tower::ServiceExt;

use mikrom_api::AppState;
use mikrom_api::repositories::PostgresAppRepository;
use mikrom_api::repositories::postgres_user_repository::PostgresUserRepository;

#[path = "common/mod.rs"]
mod common;

async fn create_app(pool: PgPool, jwt_secret: &str) -> axum::Router {
    let user_repo = PostgresUserRepository::new(pool.clone());
    let app_repo = PostgresAppRepository::new(pool.clone(), "test-key".to_string());
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();

    let mut mock_scheduler = mikrom_api::scheduler::MockScheduler::new();
    mock_scheduler
        .expect_delete_all_by_app()
        .returning(|_, _| Ok(true));
    mock_scheduler
        .expect_update_app_scaling_config()
        .returning(|_| Ok(true));

    // Simulate Router responding to NATS requests
    let nats_clone = nats_client.clone();
    tokio::spawn(async move {
        use futures::StreamExt;
        use mikrom_proto::router::RouterConfigAck;
        use prost::Message;

        if let Ok(mut sub) = nats_clone
            .subscribe(mikrom_proto::subjects::ROUTER_CONFIG_UPDATED)
            .await
        {
            while let Some(msg) = sub.next().await {
                if let Some(reply) = msg.reply {
                    let ack = RouterConfigAck {
                        success: true,
                        message: String::new(),
                    };
                    let mut buf = Vec::new();
                    if ack.encode(&mut buf).is_ok() {
                        let _ = nats_clone.publish(reply, buf.into()).await;
                    }
                }
            }
        }
    });

    let mut mock_volume_repo =
        mikrom_api::repositories::volume_repository::MockVolumeRepository::new();
    mock_volume_repo
        .expect_list_volumes_by_app()
        .returning(|_| Ok(vec![]));

    let state = AppState {
        user_repo: Arc::new(user_repo),
        app_repo: Arc::new(app_repo),
        volume_repo: Arc::new(mock_volume_repo),
        github_repo: Arc::new(mikrom_api::repositories::MockGithubRepository::default()),
        scheduler: Arc::new(mock_scheduler),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        jwt_secret: jwt_secret.to_string(),
        master_key: "integration-master-key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        api_db: pool,
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
    mikrom_api::create_app(state)
}

#[tokio::test]
async fn test_all_auth_integration_flows() {
    // Ensure rustls provider is installed
    let _ = rustls::crypto::ring::default_provider().install_default();

    let test_db = mikrom_api::test_utils::TestDb::new().await;
    let pool = test_db.pool().clone();
    let jwt_secret = "integration-test-secret";

    // Test 1: Register and Login Workflow
    {
        let app = create_app(pool.clone(), jwt_secret).await;
        let email = format!("workflow_{}@example.com", uuid::Uuid::new_v4());
        let password = "securePassword123";

        let reg_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/auth/register")
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
                    .uri("/v1/auth/login")
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
        let app = create_app(pool.clone(), jwt_secret).await;
        let email = format!("profile_{}@example.com", uuid::Uuid::new_v4());
        let password = "password123";

        // Register
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/auth/register")
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
                    .uri("/v1/auth/login")
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
                    .uri("/v1/auth/me")
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
                    .uri("/v1/auth/me")
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
        let app = create_app(pool.clone(), jwt_secret).await;
        let email = format!("multi_{}@example.com", uuid::Uuid::new_v4());

        for i in 0..2 {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/v1/auth/register")
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
        let app = create_app(pool.clone(), jwt_secret).await;
        let email = format!("hash_long_{}@example.com", uuid::Uuid::new_v4());

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/auth/register")
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
        let app = create_app(pool.clone(), jwt_secret).await;
        let email = format!("login_full_{}@example.com", uuid::Uuid::new_v4());
        let password = "password123";

        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/auth/register")
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
                    .uri("/v1/auth/login")
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
        let app = create_app(pool.clone(), jwt_secret).await;
        let email = format!("app_life_{}@example.com", uuid::Uuid::new_v4());
        let password = "password123";

        // 1. Register & Login to get token
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/auth/register")
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
                    .uri("/v1/auth/login")
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
                    .uri("/v1/apps")
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
        assert_eq!(create_resp.status(), StatusCode::CREATED);

        // 3. List Apps
        let list_resp = app
            .clone()
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
        assert_eq!(list_resp.status(), StatusCode::OK);

        // 4. Delete App
        let del_resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/v1/apps/{}", app_name))
                    .header("Authorization", format!("Bearer {}", token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);
    }
}
