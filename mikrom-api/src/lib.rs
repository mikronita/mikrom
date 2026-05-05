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

pub mod acme;
pub mod auth;
pub mod config;
pub mod crypto;
pub mod db;
pub mod deploy;
pub mod error;
pub mod models;
pub mod nats;
pub mod openapi;
pub mod repositories;
pub mod scheduler;
pub mod sync;
pub mod vms;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

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

use mikrom_proto::router::RouterConfigUpdate;

use auth::{get_profile, login, register, update_profile};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

#[derive(Clone)]
pub struct AppState {
    pub user_repo: Arc<dyn UserRepository>,
    pub app_repo: Arc<dyn AppRepository>,
    pub scheduler: Arc<dyn Scheduler>,
    pub nats: crate::nats::TypedNatsClient,
    pub router_addr: String,
    pub api_db: sqlx::PgPool,
    pub jwt_secret: String,
    pub master_key: String,
    pub deployment_events: tokio::sync::broadcast::Sender<uuid::Uuid>,
    pub acme_email: String,
    pub acme_staging: bool,
    pub acme_check_interval: u64,
}

impl AppState {
    pub async fn notify_router(&self, app: &crate::models::app::App) -> anyhow::Result<()> {
        let hostname = match &app.hostname {
            Some(h) => h,
            None => return Ok(()),
        };

        let target_url = if let Some(dep_id) = app.active_deployment_id {
            if let Some(dep) = self.app_repo.get_deployment(dep_id).await? {
                if let Some(ip) = dep.ip_address {
                    let formatted_ip = if ip.contains(':') && !ip.contains('[') {
                        format!("[{}]", ip)
                    } else {
                        ip.to_string()
                    };
                    Some(format!("http://{}:{}", formatted_ip, dep.port))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let config = RouterConfigUpdate {
            hostname: hostname.clone(),
            target_url,
            timestamp: chrono::Utc::now().timestamp(),
        };

        self.nats
            .publish(mikrom_proto::subjects::ROUTER_CONFIG_UPDATED, config)
            .await?;

        Ok(())
    }

    pub async fn reconcile_routes(&self) -> anyhow::Result<()> {
        tracing::info!("Starting route reconciliation with router...");
        let apps = self.app_repo.list_apps_by_user(None).await?;
        let mut count = 0;

        for app in apps {
            if let Err(e) = self.notify_router(&app).await {
                tracing::error!(app_id = %app.id, error = %e, "Failed to reconcile route");
            } else {
                count += 1;
            }
        }

        tracing::info!(reconciled = count, "Route reconciliation complete");
        Ok(())
    }

    pub async fn remove_route(&self, hostname: &str) -> anyhow::Result<()> {
        let config = RouterConfigUpdate {
            hostname: hostname.to_string(),
            target_url: None,
            timestamp: chrono::Utc::now().timestamp(),
        };

        self.nats
            .publish(mikrom_proto::subjects::ROUTER_CONFIG_UPDATED, config)
            .await?;

        Ok(())
    }
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
        .route("/health/stream", get(health_stream))
        .route("/auth/register", axum::routing::post(register))
        .route("/auth/login", axum::routing::post(login))
        .route(
            "/webhooks/github/:app_name",
            axum::routing::post(github_webhook_handler),
        )
        .route("/auth/me", get(get_profile))
        .route("/auth/me", axum::routing::put(update_profile))
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
            "/apps/:app_name/secret",
            get(crate::deploy::get_app_secret_handler),
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
            "/apps/:app_name/logs/stream",
            get(crate::vms::app_logs_stream_handler),
        )
        .route(
            "/apps/:app_name/metrics/stream",
            get(crate::vms::app_metrics_stream_handler),
        )
        .route(
            "/apps/:app_name/deployments/:deployment_id/activate",
            axum::routing::post(crate::deploy::activate_deployment_handler),
        )
        .route(
            "/apps/:app_name/deployments/:job_id",
            get(get_deployment_status),
        )
        .route(
            "/apps/:app_name/deployments/:job_id/logs",
            get(get_deployment_logs),
        )
        .route(
            "/apps/:app_name/deployments/:job_id/pause",
            axum::routing::post(pause_deployment),
        )
        .route(
            "/apps/:app_name/deployments/:job_id/resume",
            axum::routing::post(resume_deployment),
        )
        .route(
            "/apps/:app_name/deployments/:job_id",
            axum::routing::delete(stop_deployment),
        )
        .route(
            "/apps/:app_name/deployments/:job_id/delete",
            axum::routing::delete(delete_deployment_record),
        )
        .route("/deployments/active", get(list_active_deployments))
        .route("/deployments/events", get(watch_deployments))
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

    // Reconcile routes with router
    let state_for_router = state.clone();
    tokio::spawn(async move {
        if let Err(e) = state_for_router.reconcile_routes().await {
            tracing::error!("Route reconciliation failed: {}", e);
        }
    });

    // Start ACME certificate renewal worker
    let state_for_acme = state.clone();
    tokio::spawn(async move {
        crate::acme::start_acme_worker(
            state_for_acme.api_db.clone(),
            state_for_acme.nats.clone(),
            state_for_acme.acme_email.clone(),
            state_for_acme.acme_staging,
            state_for_acme.master_key.clone(),
            state_for_acme.acme_check_interval,
        )
        .await;
    });

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
    use mikrom_proto::scheduler::{ListAppsRequest, ListAppsResponse};

    let nats_req = ListAppsRequest {
        user_id: "system".to_string(),
        status: None,
    };

    let scheduler_res: anyhow::Result<ListAppsResponse> = state
        .nats
        .with_timeout(Duration::from_secs(2))
        .request("mikrom.scheduler.list_apps", nats_req)
        .await;

    if scheduler_res.is_ok() {
        services.insert("Scheduler".to_string(), "ONLINE".to_string());
    } else {
        services.insert("Scheduler".to_string(), "OFFLINE".to_string());
    }

    // Check Agents via NATS
    use mikrom_proto::scheduler::{ListWorkersRequest, ListWorkersResponse};
    let agents_req = ListWorkersRequest {};

    let agents_res: anyhow::Result<ListWorkersResponse> = state
        .nats
        .with_timeout(Duration::from_secs(2))
        .request("mikrom.scheduler.list_workers", agents_req)
        .await;

    match agents_res {
        Ok(workers_resp) => {
            if workers_resp.workers.is_empty() {
                services.insert("Agents".to_string(), "OFFLINE".to_string());
            } else {
                services.insert("Agents".to_string(), "ONLINE".to_string());
            }
        },
        _ => {
            services.insert("Agents".to_string(), "OFFLINE".to_string());
        },
    }

    // Check Builder via NATS
    use mikrom_proto::builder::{GetBuildStatusRequest, GetBuildStatusResponse};
    let builder_req = GetBuildStatusRequest {
        build_id: "health-check".to_string(),
    };

    let builder_res: anyhow::Result<GetBuildStatusResponse> = state
        .nats
        .with_timeout(Duration::from_secs(2))
        .request("mikrom.builder.get_status", builder_req)
        .await;

    if builder_res.is_ok() {
        services.insert("Builder".to_string(), "ONLINE".to_string());
    } else {
        services.insert("Builder".to_string(), "OFFLINE".to_string());
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
