use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

use mikrom_api::{AppState, create_app};

#[tokio::test]
async fn rollback_activate_route_requires_authentication() {
    let app = create_app(AppState::default());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/apps/test-app/deployments/test-deployment/activate")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn rollback_pause_and_resume_routes_require_authentication() {
    let app = create_app(AppState::default());

    for uri in [
        "/v1/apps/test-app/deployments/test-job/pause",
        "/v1/apps/test-app/deployments/test-job/resume",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
