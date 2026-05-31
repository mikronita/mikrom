use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mikrom_api::AppState;
use mikrom_api::create_app;
use tower::ServiceExt;

#[tokio::test]
async fn openapi_json_is_served() {
    let app = create_app(AppState::default());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/api-docs/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let spec: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(spec["openapi"], "3.1.0");
    assert_eq!(spec["info"]["title"], "Mikrom API");
    assert!(spec["paths"].get("/v1/apps").is_some());
    assert!(spec["paths"].get("/v1/health").is_some());
}

#[tokio::test]
async fn swagger_ui_is_served() {
    let app = create_app(AppState::default());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/docs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
