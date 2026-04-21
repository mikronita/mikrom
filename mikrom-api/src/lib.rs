use axum::{Router, routing::get};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Level;

pub mod auth;
pub mod config;
pub mod crypto;
pub mod db;
pub mod deploy;
pub mod error;
pub mod models;
pub mod repositories;
pub mod scheduler;
pub mod vms;

pub use deploy::deploy_app;
pub use error::{ApiError, ApiResult};
pub use repositories::user_repository::UserRepository;
pub use vms::{delete_vm, get_vm_logs, get_vm_status, list_vms, pause_vm, resume_vm, stop_vm};

use auth::{login, register};

#[derive(Clone)]
pub struct AppState {
    pub user_repo: Arc<dyn UserRepository>,
    pub scheduler_client: Option<SchedulerClient>,
    pub scheduler_config: scheduler::SchedulerConfig,
    pub jwt_secret: String,
    pub master_key: String,
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
        .route("/vms", get(list_vms))
        .route("/vms/{job_id}", get(get_vm_status))
        .route("/vms/{job_id}/logs", get(get_vm_logs))
        .route("/vms/{job_id}/pause", axum::routing::post(pause_vm))
        .route("/vms/{job_id}/resume", axum::routing::post(resume_vm))
        .route("/vms/{job_id}", axum::routing::delete(stop_vm))
        .route("/vms/{job_id}/delete", axum::routing::delete(delete_vm))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
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
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_health_endpoint() {
        let mock_repo = repositories::user_repository::MockUserRepository::new();
        // The health endpoint doesn't actually use the repo, but we need it for AppState
        let state = AppState {
            user_repo: Arc::new(mock_repo),
            scheduler_client: None,
            scheduler_config: scheduler::SchedulerConfig::default(),
            jwt_secret: "test".to_string(),
            master_key: "test".to_string(),
        };
        let app = create_app(state);

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

        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
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
