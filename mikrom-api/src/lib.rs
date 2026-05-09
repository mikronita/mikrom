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
pub mod github;
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
use deploy::webhooks::{github_webhook_handler, github_webhook_handler_generic};
pub use error::{ApiError, ApiResult};
pub use repositories::app_repository::AppRepository;
pub use repositories::github_repository::GithubRepository;
pub use repositories::user_repository::UserRepository;
pub use scheduler::Scheduler;
pub use vms::{
    create_security_rule_handler, delete_deployment_record, delete_security_rule_handler,
    get_deployment_logs, get_deployment_status, get_mesh_status_handler, list_active_deployments,
    list_security_rules_handler, pause_deployment, resume_deployment, stop_deployment,
    watch_deployments,
};

use mikrom_proto::router::RouterConfigUpdate;

use auth::{get_profile, login, register, update_profile};
use github::handlers::{github_callback, github_install, list_repos};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

#[derive(Clone)]
pub struct AppState {
    pub user_repo: Arc<dyn UserRepository>,
    pub app_repo: Arc<dyn AppRepository>,
    pub github_repo: Arc<dyn GithubRepository>,
    pub scheduler: Arc<dyn Scheduler>,
    pub nats: crate::nats::TypedNatsClient,
    pub router_addr: String,
    pub frontend_url: String,
    pub api_db: sqlx::PgPool,
    pub jwt_secret: String,
    pub master_key: String,
    pub deployment_events: tokio::sync::broadcast::Sender<uuid::Uuid>,
    pub acme_email: String,
    pub acme_staging: bool,
    pub acme_check_interval: u64,
    pub github_app_id: Option<String>,
    pub github_private_key: Option<String>,
    pub github_app_slug: Option<String>,
    pub github_webhook_url_base: Option<String>,
    pub active_deployment_flows: Arc<dashmap::DashSet<uuid::Uuid>>,
}

/// RAII guard to ensure an application's deployment flow is removed from the active set when dropped.
pub struct DeploymentFlowGuard {
    state: AppState,
    app_id: uuid::Uuid,
}

impl Drop for DeploymentFlowGuard {
    fn drop(&mut self) {
        self.state.active_deployment_flows.remove(&self.app_id);
    }
}

impl AppState {
    /// Attempts to start a deployment flow for an application.
    /// Returns a guard if successful, or None if a flow is already in progress.
    pub fn try_start_flow(&self, app_id: uuid::Uuid) -> Option<DeploymentFlowGuard> {
        if self.active_deployment_flows.insert(app_id) {
            Some(DeploymentFlowGuard {
                state: self.clone(),
                app_id,
            })
        } else {
            None
        }
    }

    pub async fn notify_router(&self, app: &crate::models::app::App) -> anyhow::Result<()> {
        let hostname = match &app.hostname {
            Some(h) => h,
            None => return Ok(()),
        };

        let target_url = if let Some(dep_id) = app.active_deployment_id {
            if let Some(dep) = self.app_repo.get_deployment(dep_id).await? {
                let ip = if let Some(ipv6) = dep.ipv6_address {
                    if !ipv6.is_empty() {
                        Some(format!("[{}]", ipv6))
                    } else {
                        dep.ip_address.clone()
                    }
                } else {
                    dep.ip_address.clone()
                };

                if let Some(ip_addr) = ip {
                    Some(format!("http://{}:{}", ip_addr, dep.port))
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

    let api_routes = Router::new()
        .route("/health", get(health))
        .route("/health/stream", get(health_stream))
        .route("/auth/register", axum::routing::post(register))
        .route("/auth/login", axum::routing::post(login))
        .route(
            "/webhooks/github/:app_name",
            axum::routing::post(github_webhook_handler),
        )
        .route(
            "/webhooks/github",
            axum::routing::post(github_webhook_handler_generic),
        )
        .route("/auth/me", get(get_profile).put(update_profile))
        .route("/github/install", get(github_install))
        .route("/github/callback", get(github_callback))
        .route("/github/repos", get(list_repos))
        .route(
            "/github/accounts",
            get(crate::github::handlers::list_accounts),
        )
        .route(
            "/apps",
            axum::routing::post(crate::deploy::create_app_handler)
                .get(crate::deploy::list_apps_handler),
        )
        .route("/deploy", axum::routing::post(crate::deploy::deploy_app))
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
            get(get_deployment_status).delete(stop_deployment),
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
            "/apps/:app_name/deployments/:job_id/delete",
            axum::routing::delete(delete_deployment_record),
        )
        .route(
            "/apps/:app_name/security-groups",
            get(list_security_rules_handler).post(create_security_rule_handler),
        )
        .route(
            "/apps/:app_name/security-groups/:rule_id",
            axum::routing::delete(delete_security_rule_handler),
        )
        .route("/networking/mesh", get(get_mesh_status_handler))
        .route("/deployments/active", get(list_active_deployments))
        .route("/deployments/events", get(watch_deployments));

    Router::new()
        .merge(SwaggerUi::new("/v1/docs").url(
            "/v1/api-docs/openapi.json",
            crate::openapi::ApiDoc::openapi(),
        ))
        .nest("/v1", api_routes)
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
    path = "/v1/health",
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
    path = "/v1/health/stream",
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
