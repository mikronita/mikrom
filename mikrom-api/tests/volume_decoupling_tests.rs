use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

use mikrom_api::{AppState, create_app};

#[tokio::test]
async fn volume_routes_require_authentication() {
    let app = create_app(AppState::default());

    for (method, uri) in [
        ("POST", "/v1/volumes"),
        ("POST", "/v1/apps/example-app/volumes/attach"),
        ("GET", "/v1/volumes/example-volume/snapshots"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
