use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

use mikrom_api::{AppState, create_app};

#[tokio::test]
async fn activate_deployment_route_requires_authentication() {
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
