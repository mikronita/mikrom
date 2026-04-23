use axum::{Router, routing::get};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Level;

pub mod auth;
pub mod builder;
pub mod config;
pub mod crypto;
pub mod db;
pub mod deploy;
pub mod error;
pub mod models;
pub mod openapi;
pub mod repositories;
pub mod scheduler;
pub mod sync;
pub mod vms;

pub use deploy::deploy_app;
pub use error::{ApiError, ApiResult};
pub use repositories::app_repository::AppRepository;
pub use repositories::user_repository::UserRepository;
pub use vms::{delete_vm, get_vm_logs, get_vm_status, list_vms, pause_vm, resume_vm, stop_vm};

use auth::{get_profile, login, register, update_profile};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

#[derive(Clone)]
pub struct AppState {
    pub user_repo: Arc<dyn UserRepository>,
    pub app_repo: Arc<dyn AppRepository>,
    pub scheduler_client: Option<SchedulerClient>,
    pub scheduler_config: scheduler::SchedulerConfig,
    pub builder_addr: String,
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
        .merge(
            SwaggerUi::new("/docs")
                .url("/api-docs/openapi.json", crate::openapi::ApiDoc::openapi()),
        )
        .route("/health", get(health))
        .route("/auth/register", axum::routing::post(register))
        .route("/auth/login", axum::routing::post(login))
        .route("/auth/me", get(get_profile))
        .route("/auth/me", axum::routing::put(update_profile))
        .route("/deploy", axum::routing::post(deploy_app))
        .route(
            "/apps",
            axum::routing::post(crate::deploy::create_app_handler),
        )
        .route("/apps", get(crate::deploy::list_apps_handler))
        .route(
            "/apps/:app_id",
            axum::routing::delete(crate::deploy::delete_app_handler),
        )
        .route(
            "/apps/:app_id/deploy",
            axum::routing::post(crate::deploy::deploy_app_version_handler),
        )
        .route(
            "/apps/:app_id/deployments",
            get(crate::deploy::list_deployments_handler),
        )
        .route("/vms", get(list_vms))
        .route("/vms/:job_id", get(get_vm_status))
        .route("/vms/:job_id/logs", get(get_vm_logs))
        .route("/vms/:job_id/pause", axum::routing::post(pause_vm))
        .route("/vms/:job_id/resume", axum::routing::post(resume_vm))
        .route("/vms/:job_id", axum::routing::delete(stop_vm))
        .route("/vms/:job_id/delete", axum::routing::delete(delete_vm))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(cors)
        .with_state(state)
}

pub fn start_background_tasks(state: AppState) {
    // Start background sync task for VM IPs
    tokio::spawn(crate::sync::start_ip_sync_task(state.clone()));

    // Resume builds that were in progress
    let state_for_builds = state;
    tokio::spawn(async move {
        crate::deploy::worker::resume_pending_builds(state_for_builds).await;
    });
}

#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "API Health Status", body = HealthResponse)
    ),
    tag = "system"
)]
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
        let db_pool = sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap();
        let app_repo = Arc::new(repositories::PostgresAppRepository::new(db_pool));

        let state = AppState {
            user_repo: Arc::new(mock_repo),
            app_repo,
            scheduler_client: None,
            scheduler_config: scheduler::SchedulerConfig::default(),
            builder_addr: "http://localhost:5004".to_string(),
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
