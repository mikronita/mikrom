use axum::extract::ConnectInfo;
use axum::response::sse::{Event, Sse};
use axum::{Router, extract::State, routing::get};
use futures::Stream;
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Level;

pub mod auth;
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
use deploy::webhooks::github_webhook_handler;
pub use error::{ApiError, ApiResult};
pub use repositories::app_repository::AppRepository;
pub use repositories::user_repository::UserRepository;
pub use scheduler::Scheduler;
pub use vms::{
    delete_deployment_record, get_deployment_logs, get_deployment_status, list_active_deployments,
    pause_deployment, resume_deployment, stop_deployment, watch_deployments,
};

use auth::{get_profile, login, register, update_profile};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

#[derive(Clone)]
pub struct AppState {
    pub user_repo: Arc<dyn UserRepository>,
    pub app_repo: Arc<dyn AppRepository>,
    pub scheduler: Arc<dyn Scheduler>,
    pub nats_client: async_nats::Client,
    pub router_addr: String,
    pub jwt_secret: String,
    pub master_key: String,
    pub deployment_events: tokio::sync::broadcast::Sender<uuid::Uuid>,
    pub build_semaphore: Arc<tokio::sync::Semaphore>,
}

impl AppState {}

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
        .route("/health/stream", get(health_stream))
        .route("/auth/register", axum::routing::post(register))
        .route("/auth/login", axum::routing::post(login))
        .route(
            "/webhooks/github/:app_name",
            axum::routing::post(github_webhook_handler),
        )
        .route("/auth/me", get(get_profile))
        .route("/auth/me", axum::routing::put(update_profile))
        .route("/deploy", axum::routing::post(deploy_app))
        .route(
            "/apps",
            axum::routing::post(crate::deploy::create_app_handler),
        )
        .route("/apps", get(crate::deploy::list_apps_handler))
        .route(
            "/apps/:app_name",
            axum::routing::delete(crate::deploy::delete_app_handler),
        )
        .route(
            "/apps/:app_name/deploy",
            axum::routing::post(crate::deploy::deploy_app_version_handler),
        )
        .route(
            "/apps/:app_name/deployments",
            get(crate::deploy::list_deployments_handler),
        )
        .route(
            "/apps/:app_name/deployments/stream",
            get(crate::deploy::deployments_stream_handler),
        )
        .route(
            "/apps/:app_name/deployments/:deployment_id/activate",
            axum::routing::post(crate::deploy::activate_deployment_handler),
        )
        .route("/deployments/active", get(list_active_deployments))
        .route("/deployments/events", get(watch_deployments))
        .route("/deployments/:job_id", get(get_deployment_status))
        .route("/deployments/:job_id/logs", get(get_deployment_logs))
        .route(
            "/deployments/:job_id/pause",
            axum::routing::post(pause_deployment),
        )
        .route(
            "/deployments/:job_id/resume",
            axum::routing::post(resume_deployment),
        )
        .route(
            "/deployments/:job_id",
            axum::routing::delete(stop_deployment),
        )
        .route(
            "/deployments/:job_id/delete",
            axum::routing::delete(delete_deployment_record),
        )
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &axum::http::Request<_>| {
                    let remote_addr = request
                        .extensions()
                        .get::<ConnectInfo<SocketAddr>>()
                        .map(|ci| ci.0.to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    tracing::info_span!(
                        "request",
                        method = %request.method(),
                        uri = %request.uri(),
                        version = ?request.version(),
                        client_ip = %remote_addr,
                    )
                })
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(cors)
        .with_state(state)
}

pub fn start_background_tasks(state: AppState) {
    // Start background sync task for VM IPs
    tokio::spawn(crate::sync::start_ip_sync_task(state.clone()));

    // Start instant NATS job updates listener
    tokio::spawn(crate::sync::start_nats_job_listener(state.clone()));

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
    pub services: HashMap<String, String>,
}

async fn get_system_health(state: &AppState) -> HashMap<String, String> {
    let mut services = HashMap::new();

    // API is always ONLINE if we are here
    services.insert("API".to_string(), "ONLINE".to_string());

    // Check Scheduler & Agents via NATS
    use mikrom_proto::scheduler::ListAppsRequest;
    use prost::Message;

    let nats_req = ListAppsRequest {
        user_id: "system".to_string(),
        status: None,
    };
    let mut buf = Vec::new();
    let payload = if nats_req.encode(&mut buf).is_ok() {
        buf
    } else {
        vec![]
    };

    let scheduler_res = tokio::time::timeout(
        Duration::from_secs(2),
        state
            .nats_client
            .request("mikrom.scheduler.list_apps", payload.into()),
    )
    .await;

    match scheduler_res {
        Ok(Ok(_)) => {
            services.insert("Scheduler".to_string(), "ONLINE".to_string());
            // In a real system, we'd check the worker registry for active agents
            services.insert("Agents".to_string(), "ONLINE".to_string());
        },
        _ => {
            services.insert("Scheduler".to_string(), "OFFLINE".to_string());
            services.insert("Agents".to_string(), "OFFLINE".to_string());
        },
    }

    // Helper function for TCP reachability check
    async fn check_tcp(addr_str: &str) -> bool {
        let clean_addr = addr_str
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_end_matches('/');

        matches!(
            tokio::time::timeout(
                Duration::from_secs(1),
                tokio::net::TcpStream::connect(clean_addr)
            )
            .await,
            Ok(Ok(_))
        )
    }

    // Check Router
    if check_tcp(&state.router_addr).await {
        services.insert("Router".to_string(), "ONLINE".to_string());
    } else {
        services.insert("Router".to_string(), "OFFLINE".to_string());
    }

    services
}

#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "API Health Status", body = HealthResponse)
    ),
    tag = "system"
)]
async fn health(State(state): State<AppState>) -> axum::Json<HealthResponse> {
    let services = get_system_health(&state).await;

    axum::Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        services,
    })
}

#[utoipa::path(
    get,
    path = "/health/stream",
    responses(
        (status = 200, description = "SSE stream of System Health Updates"),
    ),
    tag = "system"
)]
async fn health_stream(
    State(state): State<AppState>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let stream = async_stream::stream! {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            let services = get_system_health(&state).await;

            let response = HealthResponse {
                status: "ok".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                services,
            };

            if let Ok(data) = serde_json::to_string(&response) {
                yield Ok(Event::default().data(data));
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_response_serialization() {
        let mut services = HashMap::new();
        services.insert("API".to_string(), "ONLINE".to_string());
        let response = HealthResponse {
            status: "ok".to_string(),
            version: "1.0.0".to_string(),
            services,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("ok"));
        assert!(json.contains("1.0.0"));
        assert!(json.contains("ONLINE"));
    }
}
