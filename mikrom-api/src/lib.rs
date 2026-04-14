use axum::{
    routing::get,
    Router,
};
use tower_http::cors::{Any, CorsLayer};

pub mod auth;
pub mod db;
pub mod models;
pub mod deploy;

pub use deploy::deploy_app;

use auth::{login, register};

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::PgPool,
    pub scheduler_client: Option<SchedulerClient>,
}

#[derive(Clone)]
pub struct SchedulerClient {
    pub channel: tonic::transport::Channel,
}

pub fn create_app(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/health", get(health))
        .route("/auth/register", axum::routing::post(register))
        .route("/auth/login", axum::routing::post(login))
        .route("/deploy", axum::routing::post(deploy_app))
        .layer(cors)
        .with_state(state)
}

#[derive(Clone, serde::Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

async fn health() -> axum::Json<HealthResponse> {
    axum::Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::{Request, StatusCode}};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_health_endpoint() {
        let app = Router::new().route("/health", get(health));

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
        assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn test_health_response_serialization() {
        let response = HealthResponse {
            status: "ok".to_string(),
            version: "1.0.0".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("ok"));
        assert!(json.contains("1.0.0"));
    }
}