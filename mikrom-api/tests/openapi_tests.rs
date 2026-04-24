use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mikrom_api::AppState;
use mikrom_api::create_app;
use mikrom_api::repositories::{MockAppRepository, MockUserRepository};
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test]
async fn test_openapi_docs_endpoint() {
    let mock_user_repo = MockUserRepository::new();
    let mock_app_repo = MockAppRepository::new();

    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        scheduler_client: None,
        scheduler_config: Default::default(),
        builder_addr: "http://localhost:5004".into(),
        jwt_secret: "test-secret".into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
    };

    let router = create_app(state);

    // Test that /docs redirects or serves the UI
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/docs/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // utoipa-swagger-ui usually returns 200 for the index file
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_openapi_json_spec() {
    let mock_user_repo = MockUserRepository::new();
    let mock_app_repo = MockAppRepository::new();

    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        scheduler_client: None,
        scheduler_config: Default::default(),
        builder_addr: "http://localhost:5004".into(),
        jwt_secret: "test-secret".into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
    };

    let router = create_app(state);

    // Test that the JSON spec is served correctly
    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api-docs/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 1024 * 100)
        .await
        .unwrap();
    let spec: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Verify some key parts of the spec
    assert_eq!(spec["openapi"], "3.1.0");
    assert!(spec["paths"]["/auth/login"].is_object());
    assert!(spec["paths"]["/auth/register"].is_object());
    assert!(spec["paths"]["/apps"].is_object());
    assert!(spec["paths"]["/health"].is_object());
    assert!(spec["paths"]["/deployments/active"].is_object());
    assert!(spec["paths"]["/deployments/{job_id}"].is_object());
    assert!(spec["paths"]["/deployments/{job_id}/logs"].is_object());
    assert!(spec["paths"]["/deployments/{job_id}/pause"].is_object());
    assert!(spec["paths"]["/deployments/{job_id}/resume"].is_object());
    assert!(spec["paths"]["/deployments/{job_id}/delete"].is_object());
    assert!(spec["components"]["schemas"]["App"].is_object());
    assert!(spec["components"]["schemas"]["Deployment"].is_object());
    assert!(spec["components"]["schemas"]["HealthResponse"].is_object());
}
