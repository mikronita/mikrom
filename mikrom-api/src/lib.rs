use std::sync::Arc;

use mikrom_proto::router::RouterConfigAck;
use mikrom_proto::router::RouterConfigUpdate;

pub mod application;
pub mod config;
pub mod domain;
pub mod error;
pub mod infrastructure;
pub mod openapi;
pub mod sync;
pub mod workspace;

pub use infrastructure::acme;
pub use infrastructure::auth::{self, extractor::AuthUser};
pub use infrastructure::crypto;
pub use infrastructure::http::rate_limit;
pub use infrastructure::nats;
pub use infrastructure::scheduler::{NatsScheduler, Scheduler, status_name};

pub mod test_utils;

pub use domain::{
    AppRepository, DatabaseRepository, GithubRepository, TenantRepository, UserRepository,
    VolumeRepository,
};
pub use error::{ApiError, ApiResult};

use crate::application::vms::MeshStatus;

pub use workspace::{WorkspaceEvent, WorkspaceEventKind};

pub fn normalize_loopback_url(url: &str) -> String {
    for (prefix, replacement) in [
        ("http://[::1]", "http://localhost"),
        ("https://[::1]", "https://localhost"),
        ("http://[0:0:0:0:0:0:0:1]", "http://localhost"),
        ("https://[0:0:0:0:0:0:0:1]", "https://localhost"),
    ] {
        if let Some(suffix) = url.strip_prefix(prefix) {
            return format!("{replacement}{suffix}");
        }
    }

    url.to_string()
}

#[must_use]
pub fn normalize_app_slug(name: &str) -> Option<String> {
    let slug = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|ch| match ch {
            'a'..='z' | '0'..='9' => ch,
            _ => '-',
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    (!slug.is_empty()).then_some(slug)
}

#[must_use]
pub fn build_app_hostname(name: &str) -> ApiResult<String> {
    let slug = normalize_app_slug(name).ok_or_else(|| {
        ApiError::BadRequest(
            "Application name must contain at least one alphanumeric character".to_string(),
        )
    })?;

    Ok(format!("{}.apps.mikrom.spluca.org", slug))
}

#[cfg(test)]
mod tests {
    use super::{build_app_hostname, normalize_app_slug};

    #[test]
    fn normalize_app_slug_collapses_invalid_characters() {
        assert_eq!(
            normalize_app_slug(" My App! 1 "),
            Some("my-app-1".to_string())
        );
    }

    #[test]
    fn normalize_app_slug_rejects_empty_values() {
        assert_eq!(normalize_app_slug("   "), None);
    }

    #[test]
    fn build_app_hostname_appends_platform_domain() {
        assert_eq!(
            build_app_hostname(" My App! 1 ").unwrap(),
            "my-app-1.apps.mikrom.spluca.org"
        );
    }

    #[test]
    fn build_app_hostname_rejects_empty_values() {
        let err = build_app_hostname("   ").unwrap_err();
        assert!(matches!(err, crate::ApiError::BadRequest(_)));
    }
}

#[derive(Clone)]
pub struct AppState {
    pub ctx: crate::application::ApiContext,
    pub user_repo: Arc<dyn UserRepository>,
    pub tenant_repo: Arc<dyn TenantRepository>,
    pub app_repo: Arc<dyn AppRepository>,
    pub database_repo: Arc<dyn DatabaseRepository>,
    pub github_repo: Arc<dyn GithubRepository>,
    pub volume_repo: Arc<dyn VolumeRepository>,
    pub scheduler: Arc<dyn Scheduler>,
    pub nats: infrastructure::nats::TypedNatsClient,
    pub router_addr: String,
    pub frontend_url: String,
    pub api_db: sqlx::PgPool,
    pub jwt_secret: String,
    pub master_key: String,
    pub deployment_events: tokio::sync::broadcast::Sender<uuid::Uuid>,
    pub workspace_events: tokio::sync::broadcast::Sender<WorkspaceEvent>,
    pub mesh_status: tokio::sync::watch::Sender<MeshStatus>,
    pub acme_email: String,
    pub acme_staging: bool,
    pub acme_check_interval: u64,
    pub github_app_id: Option<String>,
    pub github_private_key: Option<String>,
    pub github_app_slug: Option<String>,
    pub github_webhook_url_base: Option<String>,
    pub active_deployment_flows: Arc<dashmap::DashSet<mikrom_proto::id::AppId>>,
}

impl Default for AppState {
    fn default() -> Self {
        let ctx = crate::application::ApiContext::default();
        let (deployment_events, _) = tokio::sync::broadcast::channel(32);
        let (workspace_events, _) = tokio::sync::broadcast::channel(32);
        let (mesh_status, _) = tokio::sync::watch::channel(MeshStatus::default());

        Self {
            user_repo: ctx.user_repo.clone(),
            tenant_repo: ctx.tenant_repo.clone(),
            app_repo: ctx.app_repo.clone(),
            database_repo: ctx.database_repo.clone(),
            github_repo: ctx.github_repo.clone(),
            volume_repo: ctx.volume_repo.clone(),
            scheduler: ctx.scheduler.clone(),
            nats: ctx.nats.clone(),
            router_addr: ctx.config.router_addr.clone(),
            frontend_url: ctx.config.frontend_url.clone(),
            api_db: ctx.db.clone(),
            jwt_secret: ctx.jwt_secret.clone(),
            master_key: ctx.master_key.clone(),
            deployment_events,
            workspace_events,
            mesh_status,
            acme_email: ctx.config.acme_email.clone(),
            acme_staging: ctx.config.acme_staging,
            acme_check_interval: ctx.config.acme_check_interval,
            github_app_id: ctx.config.github_app_id.clone(),
            github_private_key: ctx.config.github_private_key.clone(),
            github_app_slug: ctx.config.github_app_slug.clone(),
            github_webhook_url_base: ctx.config.github_webhook_url_base.clone(),
            active_deployment_flows: Arc::new(dashmap::DashSet::new()),
            ctx,
        }
    }
}

/// RAII guard to ensure an application's deployment flow is removed from the active set when dropped.
pub struct DeploymentFlowGuard {
    state: AppState,
    app_id: mikrom_proto::id::AppId,
}

impl Drop for DeploymentFlowGuard {
    fn drop(&mut self) {
        self.state.active_deployment_flows.remove(&self.app_id);
    }
}

impl AppState {
    pub fn nats_request_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.ctx.config.nats_request_timeout_secs.max(1))
    }

    /// Attempts to start a deployment flow for an application.
    /// Returns a guard if successful, or None if a flow is already in progress.
    pub fn try_start_flow(&self, app_id: mikrom_proto::id::AppId) -> Option<DeploymentFlowGuard> {
        if self.active_deployment_flows.insert(app_id) {
            Some(DeploymentFlowGuard {
                state: self.clone(),
                app_id,
            })
        } else {
            None
        }
    }

    pub async fn notify_router(&self, app: &crate::domain::App) -> anyhow::Result<()> {
        let hostname = match &app.hostname {
            Some(h) => h,
            None => return Ok(()),
        };

        let mut target_urls = Vec::new();

        // Get all running deployments (replicas) for this app
        let jobs = self
            .scheduler
            .list_apps(mikrom_proto::scheduler::ListAppsRequest {
                tenant_id: app.tenant_id.to_string(),
                status: Some(mikrom_proto::scheduler::DeployStatus::Running as i32),
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list running jobs from scheduler: {}", e))?;

        for job in jobs.apps {
            if job.app_id == app.id.to_string() && !job.ipv6_address.is_empty() {
                // Determine port: prefer deployment port if available, fallback to app port
                let port = if let Some(dep_id) = app.active_deployment_id {
                    if let Ok(Some(dep)) = self.app_repo.get_deployment(dep_id).await {
                        dep.port
                    } else {
                        app.port
                    }
                } else {
                    app.port
                };

                target_urls.push(format!("[{}]:{}", job.ipv6_address, port));
            }
        }

        let has_targets = !target_urls.is_empty();
        let config = RouterConfigUpdate {
            hostname: hostname.clone(),
            target_urls,
            timestamp: chrono::Utc::now().timestamp(),
        };

        let ack: RouterConfigAck = self
            .nats
            .with_timeout(self.nats_request_timeout())
            .request(mikrom_proto::subjects::ROUTER_CONFIG_UPDATED, config)
            .await?;

        if !ack.success {
            return Err(anyhow::anyhow!(
                "router rejected route update for {}: {}",
                hostname,
                ack.message
            ));
        }

        if has_targets {
            // Resolve the tenant's VPC prefix from one of its members.
            // The prefix is stored on the user record and shared across the tenant.
            let members = self
                .tenant_repo
                .get_members(app.tenant_id)
                .await
                .unwrap_or_default();
            let vpc_ipv6_prefix = if let Some(first_member) = members.first() {
                self.user_repo
                    .find_by_id(first_member.user_id)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|u| u.vpc_ipv6_prefix)
                    .unwrap_or_default()
            } else {
                String::new()
            };

            if let Err(err) = self
                .scheduler
                .update_app_scaling_config(mikrom_proto::scheduler::UpdateAppScalingConfigRequest {
                    app_id: app.id.to_string(),
                    tenant_id: app.tenant_id.to_string(),
                    min_replicas: app.min_replicas as u32,
                    max_replicas: app.max_replicas as u32,
                    autoscaling_enabled: app.autoscaling_enabled,
                    cpu_threshold: app.cpu_threshold,
                    mem_threshold: app.mem_threshold,
                    vpc_ipv6_prefix,
                    desired_replicas: app.desired_replicas as u32,
                    hostname: app.hostname.clone().unwrap_or_default(),
                    last_router_traffic_at: chrono::Utc::now().timestamp(),
                    last_scaled_to_zero_at: 0,
                })
                .await
            {
                tracing::warn!(
                    app_id = %app.id,
                    error = %err,
                    "Failed to sync scaling config with scheduler while notifying router"
                );
            }
        }

        Ok(())
    }

    pub async fn reconcile_routes(&self) -> anyhow::Result<()> {
        tracing::info!("Starting route reconciliation with router...");
        let apps = self.app_repo.list_apps_by_tenant(None).await?;
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
            target_urls: vec![],
            timestamp: chrono::Utc::now().timestamp(),
        };

        let ack: RouterConfigAck = self
            .nats
            .with_timeout(self.nats_request_timeout())
            .request(mikrom_proto::subjects::ROUTER_CONFIG_UPDATED, config)
            .await
            .map_err(|e| anyhow::anyhow!("failed to request route removal: {}", e))?;

        if !ack.success {
            return Err(anyhow::anyhow!(
                "router rejected route removal for {}: {}",
                hostname,
                ack.message
            ));
        }

        Ok(())
    }

    pub fn publish_workspace_event(&self, event: WorkspaceEvent) {
        let state = self.clone();
        tokio::spawn(async move {
            let projection_event = event.clone();
            if let Err(err) =
                crate::application::notifications::project_workspace_event(&state, projection_event)
                    .await
            {
                tracing::error!(error = %err, "Failed to project workspace notification");
            }

            let _ = state.workspace_events.send(event);
        });
    }
}

pub const API_V1: &str = "/v1";
pub const OPENAPI_PATH: &str = "/v1/api-docs/openapi";
pub const SWAGGER_PATH: &str = "/v1/docs";

pub fn create_app(state: AppState) -> axum::Router {
    let rate_limiter = Arc::new(
        crate::rate_limit::RateLimiter::new(
            crate::rate_limit::RateLimitConfig::default(),
            state.jwt_secret.clone(),
        )
        .expect("default rate limit config must be valid"),
    );
    create_app_with_rate_limits(state, rate_limiter)
}

pub fn create_app_with_rate_limits(
    state: AppState,
    rate_limiter: Arc<crate::rate_limit::RateLimiter>,
) -> axum::Router {
    infrastructure::http::routes::create_app_with_rate_limits(state, rate_limiter)
}

pub fn start_background_tasks(state: AppState) {
    // Start background sync task for VM IPs
    tokio::spawn(crate::sync::start_ip_sync_task(state.clone()));

    // Start instant NATS job updates listener
    tokio::spawn(crate::sync::start_nats_job_listener(state.clone()));

    // Track mesh status centrally and fan out updates to clients.
    tokio::spawn(crate::application::vms::start_mesh_status_tracker(
        state.clone(),
    ));

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
            state_for_acme.ctx.config.router_tls_hostname.clone(),
            state_for_acme.master_key.clone(),
            state_for_acme.acme_check_interval,
            state_for_acme.router_addr.clone(),
        )
        .await;
    });

    // Resume builds that were in progress
    let state_for_builds = state;
    tokio::spawn(async move {
        crate::application::deployment::worker::resume_pending_builds(state_for_builds).await;
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_health_response_serialization() {
        let mut services = std::collections::HashMap::new();
        services.insert("API".to_string(), "ONLINE".to_string());
        let response = crate::infrastructure::http::health::HealthResponse {
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
