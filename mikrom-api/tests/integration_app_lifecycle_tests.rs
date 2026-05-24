#![cfg(feature = "api-e2e")]

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

#[path = "common/mod.rs"]
mod common;

#[tokio::test]
async fn test_app_lifecycle_integration_flow() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let test_db = mikrom_api::test_utils::TestDb::new().await;
    let pool = test_db.pool().clone();
    let jwt_secret = "integration-test-secret";

    let Some(app) = common::integration::create_integration_app(pool.clone(), jwt_secret).await
    else {
        return;
    };
    let email = format!("app_life_{}@example.com", uuid::Uuid::new_v4());
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
