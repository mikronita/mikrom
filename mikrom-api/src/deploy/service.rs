use crate::AppState;
use crate::deploy::worker::{BuildTask, start_build_polling};
use crate::deploy::workflow::DeploymentPromotionWorkflow;
use crate::error::{ApiError, ApiResult};
use crate::models::app::{App, Deployment};
use crate::repositories::app_repository::UpdateDeploymentParams;
use crate::workspace::{WorkspaceEvent, WorkspaceEventKind};
use mikrom_proto::scheduler::{AppConfig, DeployRequest, DeployResponse};

pub struct DeploymentService;

const DEFAULT_ZERO_DOWNTIME_HEALTH_CHECK_MAX_ATTEMPTS: usize = 45;
const DEFAULT_ZERO_DOWNTIME_HEALTH_CHECK_REQUEST_TIMEOUT_SECS: u64 = 2;
pub(crate) const ZERO_DOWNTIME_HEALTH_CHECK_RETRY_DELAY_SECS: u64 = 1;

pub struct DeployParams {
    pub image_tag: String,
    pub vcpus: u32,
    pub memory_mib: u32,
    pub disk_mib: u32,
    pub env: std::collections::HashMap<String, String>,
}

impl DeploymentService {
    fn parse_usize_env(value: Option<String>, default: usize) -> usize {
        value
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(default)
    }

    fn parse_u64_env(value: Option<String>, default: u64) -> u64 {
        value
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(default)
    }

    pub(crate) fn zero_downtime_health_check_max_attempts() -> usize {
        Self::parse_usize_env(
            std::env::var("MIKROM_ZERO_DOWNTIME_HEALTH_CHECK_MAX_ATTEMPTS").ok(),
            DEFAULT_ZERO_DOWNTIME_HEALTH_CHECK_MAX_ATTEMPTS,
        )
    }

    pub(crate) fn zero_downtime_health_check_request_timeout() -> std::time::Duration {
        let secs = Self::parse_u64_env(
            std::env::var("MIKROM_ZERO_DOWNTIME_HEALTH_CHECK_TIMEOUT_SECS").ok(),
            DEFAULT_ZERO_DOWNTIME_HEALTH_CHECK_REQUEST_TIMEOUT_SECS,
        );

        std::time::Duration::from_secs(secs)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn trigger_build(
        state: &AppState,
        app: &App,
        deployment: &Deployment,
        vcpus: u32,
        memory_mib: u64,
        disk_mib: u64,
        env: std::collections::HashMap<String, String>,
        guard: crate::DeploymentFlowGuard,
    ) -> ApiResult<String> {
        state
            .app_repo
            .update_deployment(
                deployment.id,
                UpdateDeploymentParams {
                    status: Some("BUILDING".to_string()),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        // Notify cluster via NATS for BUILDING phase
        {
            use mikrom_proto::scheduler::AppInfo;
            let info = AppInfo {
                job_id: format!("temp-{}", deployment.id),
                app_id: app.id.to_string(),
                app_name: app.name.clone(),
                image: String::new(),
                status: 1, // Pending/Building
                user_id: app.user_id.to_string(),
                deployment_id: deployment.id.to_string(),
                ..Default::default()
            };
            let _ = state
                .nats
                .publish("mikrom.scheduler.job_updates", info)
                .await;
        }

        state.deployment_events.send(app.id).ok();
        state.publish_workspace_event(WorkspaceEvent {
            kind: WorkspaceEventKind::DeploymentChanged,
            user_id: Some(app.user_id),
            app_id: Some(app.id),
            app_name: Some(app.name.clone()),
            deployment_id: Some(deployment.id),
            resource_id: Some(deployment.id.to_string()),
        });

        let mut git_auth_token = None;
        if let (Some(installation_id), Some(app_id), Some(private_key)) = (
            app.github_installation_id,
            &state.github_app_id,
            &state.github_private_key,
        ) {
            match crate::github::get_installation_token(app_id, private_key, installation_id).await
            {
                Ok(token) => git_auth_token = Some(token),
                Err(e) => tracing::error!("Failed to get GitHub installation token: {}", e),
            }
        }

        let build_req = mikrom_proto::builder::BuildRequest {
            app_id: app.id.to_string(),
            git_url: app.git_url.clone(),
            image_name: app.name.to_lowercase().replace(' ', "-"),
            tag: deployment.id.to_string(),
            git_auth_token,
        };

        let build_resp: mikrom_proto::builder::BuildResponse = state
            .nats
            .with_timeout(std::time::Duration::from_secs(5))
            .request("mikrom.builder.build", build_req)
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to trigger build via NATS: {}", e)))?;

        let build_id = build_resp.build_id;
        state
            .app_repo
            .update_deployment(
                deployment.id,
                UpdateDeploymentParams {
                    status: Some("BUILDING".to_string()),
                    build_id: Some(build_id.clone()),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        let task = BuildTask {
            deployment_id: deployment.id,
            app_id: app.id,
            app_name: app.name.clone(),
            user_id: app.user_id.to_string(),
            build_id: build_id.clone(),
            vcpus,
            memory_mib,
            disk_mib,
            port: app.port as u32,
            env,
        };

        start_build_polling(state.clone(), task, Some(guard)).await;

        Ok(build_id)
    }

    pub async fn deploy_to_scheduler(
        state: &AppState,
        app: &App,
        deployment: &Deployment,
        params: DeployParams,
    ) -> ApiResult<DeployResponse> {
        state
            .app_repo
            .update_deployment(
                deployment.id,
                UpdateDeploymentParams {
                    status: Some("SCHEDULED".to_string()),
                    image_tag: Some(params.image_tag.clone()),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        let user = state
            .user_repo
            .find_by_id(app.user_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

        let vpc_ipv6_prefix = user.vpc_ipv6_prefix.unwrap_or_default();

        let volumes = state
            .volume_repo
            .list_volumes_by_app(app.id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        let nats_req = DeployRequest {
            app_id: app.id.to_string(),
            app_name: app.name.clone(),
            image: params.image_tag.clone(),
            user_id: app.user_id.to_string(),
            config: Some(AppConfig {
                vcpus: params.vcpus,
                memory_mib: params.memory_mib,
                disk_mib: params.disk_mib,
                port: app.port as u32,
                env: params.env,
                health_check_path: app.health_check_path.clone(),
                volumes: volumes
                    .into_iter()
                    .map(|v| mikrom_proto::scheduler::Volume {
                        volume_id: v.id.to_string(),
                        size_mib: v.size_mib as u64,
                        read_only: false, // Default to RW
                        pool_name: v.pool_name,
                    })
                    .collect(),
                ..Default::default()
            }),
            deployment_id: deployment.id.to_string(),
            vpc_ipv6_prefix,
        };

        let nats_result: ApiResult<DeployResponse> = state
            .nats
            .with_timeout(std::time::Duration::from_secs(5))
            .request("mikrom.scheduler.deploy", nats_req)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()));

        let inner = match nats_result {
            Ok(inner) => inner,
            Err(e) => {
                let _ = state
                    .app_repo
                    .update_deployment(
                        deployment.id,
                        UpdateDeploymentParams {
                            status: Some("FAILED".to_string()),
                            image_tag: Some(params.image_tag.clone()),
                            git_branch: Some(e.to_string()),
                            ..Default::default()
                        },
                    )
                    .await;
                state.deployment_events.send(app.id).ok();
                state.publish_workspace_event(WorkspaceEvent {
                    kind: WorkspaceEventKind::DeploymentChanged,
                    user_id: Some(app.user_id),
                    app_id: Some(app.id),
                    app_name: Some(app.name.clone()),
                    deployment_id: Some(deployment.id),
                    resource_id: Some(deployment.id.to_string()),
                });
                return Err(e);
            },
        };

        let db_status = crate::scheduler::status_name(inner.status);
        state
            .app_repo
            .update_deployment(
                deployment.id,
                UpdateDeploymentParams {
                    status: Some(db_status.to_string()),
                    job_id: Some(inner.job_id.clone()),
                    image_tag: Some(params.image_tag),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        state.deployment_events.send(app.id).ok();
        state.publish_workspace_event(WorkspaceEvent {
            kind: WorkspaceEventKind::DeploymentChanged,
            user_id: Some(app.user_id),
            app_id: Some(app.id),
            app_name: Some(app.name.clone()),
            deployment_id: Some(deployment.id),
            resource_id: Some(deployment.id.to_string()),
        });

        Ok(inner)
    }

    pub fn run_zero_downtime_flow(
        state: crate::AppState,
        app: crate::models::app::App,
        deployment: crate::models::app::Deployment,
        inner: mikrom_proto::scheduler::DeployResponse,
        user_id: String,
        cleanup_on_failure: bool,
        _guard: crate::DeploymentFlowGuard,
    ) {
        DeploymentPromotionWorkflow::run_zero_downtime_flow(
            state,
            app,
            deployment,
            inner,
            user_id,
            cleanup_on_failure,
            _guard,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn zero_downtime_defaults_cover_init_window() {
        assert!(
            DEFAULT_ZERO_DOWNTIME_HEALTH_CHECK_MAX_ATTEMPTS >= 35,
            "Default zero-downtime health-check attempts should exceed mikrom-init's 30s wait"
        );
        assert_eq!(DEFAULT_ZERO_DOWNTIME_HEALTH_CHECK_REQUEST_TIMEOUT_SECS, 2);
    }

    #[test]
    fn zero_downtime_env_parsing_falls_back_on_invalid_values() {
        assert_eq!(DeploymentService::parse_usize_env(None, 9), 9);
        assert_eq!(
            DeploymentService::parse_usize_env(Some("7".to_string()), 9),
            7
        );
        assert_eq!(
            DeploymentService::parse_usize_env(Some("not-a-number".to_string()), 9),
            9
        );

        assert_eq!(DeploymentService::parse_u64_env(None, 11), 11);
        assert_eq!(
            DeploymentService::parse_u64_env(Some("13".to_string()), 11),
            13
        );
        assert_eq!(
            DeploymentService::parse_u64_env(Some("bad".to_string()), 11),
            11
        );
    }
}
