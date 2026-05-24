#![cfg(feature = "api-e2e")]

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

#[path = "common/mod.rs"]
mod common;

#[tokio::test]
async fn test_all_auth_integration_flows() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let test_db = mikrom_api::test_utils::TestDb::new().await;
    let pool = test_db.pool().clone();
    let jwt_secret = "integration-test-secret";

    {
        let Some(app) = common::integration::create_integration_app(pool.clone(), jwt_secret).await
        else {
            return;
        };
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

    {
        let Some(app) = common::integration::create_integration_app(pool.clone(), jwt_secret).await
        else {
            return;
        };
        let email = format!("profile_{}@example.com", uuid::Uuid::new_v4());
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

    {
        let Some(app) = common::integration::create_integration_app(pool.clone(), jwt_secret).await
        else {
            return;
        };
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

    {
        let Some(app) = common::integration::create_integration_app(pool.clone(), jwt_secret).await
        else {
            return;
        };
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

    {
        let Some(app) = common::integration::create_integration_app(pool.clone(), jwt_secret).await
        else {
            return;
        };
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
}
