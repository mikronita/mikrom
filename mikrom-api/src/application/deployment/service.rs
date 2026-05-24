use super::worker::{BuildTask, start_build_polling};
use super::workflow::DeploymentPromotionWorkflow;
use crate::AppState;
use crate::domain::types::{CpuCores, MemoryMb, Port};
use crate::domain::{App, Deployment, UpdateDeploymentParams, VolumeAccessMode};
use crate::error::{ApiError, ApiResult};
use crate::workspace::{WorkspaceEvent, WorkspaceEventKind};
use mikrom_proto::scheduler::{AppConfig, DeployRequest, DeployResponse};
use std::collections::HashMap;
use std::convert::TryFrom;
use uuid::Uuid;

pub struct DeploymentService;

const DEFAULT_ZERO_DOWNTIME_HEALTH_CHECK_MAX_ATTEMPTS: usize = 45;
const DEFAULT_ZERO_DOWNTIME_HEALTH_CHECK_REQUEST_TIMEOUT_SECS: u64 = 2;
pub(crate) const ZERO_DOWNTIME_HEALTH_CHECK_RETRY_DELAY_SECS: u64 = 1;

pub struct DeployParams {
    pub image_tag: String,
    pub vcpus: CpuCores,
    pub memory_mib: MemoryMb,
    pub disk_mib: u32,
    pub port: Port,
    pub env: std::collections::HashMap<String, String>,
    pub hypervisor: i32,
}

pub struct TriggerBuildParams {
    pub vcpus: CpuCores,
    pub memory_mib: u64,
    pub disk_mib: u64,
    pub env: std::collections::HashMap<String, String>,
    pub hypervisor: i32,
    pub guard: crate::DeploymentFlowGuard,
}

#[derive(Clone)]
pub struct DeployVersionParams {
    pub vcpus: CpuCores,
    pub memory_mib: MemoryMb,
    pub disk_mib: u32,
    pub env: HashMap<String, String>,
    pub image: Option<String>,
    pub hypervisor: i32,
}

pub struct ScaleAppParams {
    pub desired_replicas: Option<i32>,
    pub min_replicas: Option<i32>,
    pub max_replicas: Option<i32>,
    pub autoscaling_enabled: Option<bool>,
    pub cpu_threshold: Option<f64>,
    pub mem_threshold: Option<f64>,
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

    pub async fn create_app(
        state: &AppState,
        params: crate::domain::CreateAppParams,
    ) -> ApiResult<App> {
        let user_id = params.user_id;
        let user = state
            .user_repo
            .find_by_id(user_id)
            .await?
            .ok_or_else(|| ApiError::NotFound("User not found".into()))?;

        let app = state.app_repo.create_app(params).await?;

        state.publish_workspace_event(WorkspaceEvent {
            kind: WorkspaceEventKind::AppCreated,
            user_id: Some(user_id),
            app_id: Some(app.id),
            app_name: Some(app.name.clone()),
            deployment_id: app.active_deployment_id,
            volume_id: None,
            resource_id: None,
        });

        // Notify Scheduler about initial scaling config
        if let Err(err) = state
            .scheduler
            .update_app_scaling_config(mikrom_proto::scheduler::UpdateAppScalingConfigRequest {
                app_id: app.id.to_string(),
                user_id: app.user_id.to_string(),
                min_replicas: app.min_replicas as u32,
                max_replicas: app.max_replicas as u32,
                autoscaling_enabled: app.autoscaling_enabled,
                cpu_threshold: app.cpu_threshold,
                mem_threshold: app.mem_threshold,
                vpc_ipv6_prefix: user.vpc_ipv6_prefix.clone().unwrap_or_default(),
                desired_replicas: app.desired_replicas as u32,
                hostname: app.hostname.clone().unwrap_or_default(),
                last_router_traffic_at: 0,
                last_scaled_to_zero_at: 0,
            })
            .await
        {
            tracing::warn!(
                app_id = %app.id,
                error = %err,
                "Failed to sync initial scaling config with scheduler"
            );
        }

        // Trigger immediate ACME certification if hostname is present
        if let Some(hostname) = &app.hostname {
            let state_for_acme = state.clone();
            let hostname = hostname.clone();
            tokio::spawn(async move {
                if let Err(e) =
                    crate::acme::trigger_domain_certification(&state_for_acme, &hostname).await
                {
                    tracing::error!(hostname = %hostname, error = %e, "Immediate ACME certification on app creation failed");
                }
            });
        }

        Ok(app)
    }

    pub async fn maybe_create_github_webhook(
        state: &AppState,
        app: &App,
        webhook_secret: &str,
    ) -> ApiResult<()> {
        let (
            Some(installation_id),
            Some(repo_full_name),
            Some(github_app_id),
            Some(github_private_key),
        ) = (
            app.github_installation_id,
            app.github_repo_full_name.as_ref(),
            state.github_app_id.as_ref(),
            state.github_private_key.as_ref(),
        )
        else {
            return Ok(());
        };

        let Some(base) = &state.github_webhook_url_base else {
            return Err(ApiError::BadRequest(
                "GITHUB_WEBHOOK_URL_BASE must be configured to create GitHub webhooks".into(),
            ));
        };

        let webhook_url = if base.contains("smee.io") {
            base.to_string()
        } else {
            format!(
                "{}/v1/webhooks/github/{}",
                base.trim_end_matches('/'),
                app.name
            )
        };

        let github_app_id = github_app_id.clone();
        let github_private_key = github_private_key.clone();
        let repo_full_name = repo_full_name.clone();
        let webhook_secret = webhook_secret.to_string();
        let app_id = app.id;
        let app_name = app.name.clone();

        tokio::spawn(async move {
            if let Err(e) = crate::infrastructure::github::create_repository_webhook(
                &github_app_id,
                &github_private_key,
                installation_id,
                &repo_full_name,
                &webhook_url,
                &webhook_secret,
            )
            .await
            {
                tracing::error!(%app_id, error = %e, "Failed to automatically create GitHub webhook");
            } else {
                tracing::info!(app_name = %app_name, "Successfully created automatic GitHub webhook");
            }
        });

        Ok(())
    }

    pub async fn trigger_immediate_acme_certification(state: &AppState, app: &App) {
        let Some(hostname) = &app.hostname else {
            return;
        };

        let state_for_acme = state.clone();
        let hostname = hostname.clone();
        tokio::spawn(async move {
            if let Err(e) =
                crate::acme::trigger_domain_certification(&state_for_acme, &hostname).await
            {
                tracing::error!(hostname = %hostname, error = %e, "Immediate ACME certification on app creation failed");
            }
        });
    }

    pub async fn get_app_by_name_and_auth(
        state: &AppState,
        app_name: &str,
        auth_user_id: &str,
    ) -> ApiResult<App> {
        let app = state
            .app_repo
            .get_app_by_name(app_name)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or_else(|| ApiError::NotFound("Application not found".into()))?;

        if app.user_id.to_string() != auth_user_id {
            return Err(ApiError::Forbidden);
        }

        Ok(app)
    }

    pub async fn fetch_app_git_metadata(
        state: &AppState,
        app: &App,
    ) -> Option<crate::domain::GitMetadata> {
        let installation_id = app.github_installation_id?;
        let repo_full_name = app.github_repo_full_name.as_ref()?;
        let (github_app_id, github_private_key) = match (
            &state.github_app_id,
            &state.github_private_key,
        ) {
            (Some(id), Some(key)) => (id, key),
            _ => {
                tracing::warn!(app_id = %app.id, "GitHub linked but API credentials missing in state");
                return None;
            },
        };

        match crate::infrastructure::github::get_repo_latest_commit(
            github_app_id,
            github_private_key,
            installation_id,
            repo_full_name,
            "main",
        )
        .await
        {
            Ok(meta) => Some(meta),
            Err(_) => crate::infrastructure::github::get_repo_latest_commit(
                github_app_id,
                github_private_key,
                installation_id,
                repo_full_name,
                "master",
            )
            .await
            .ok(),
        }
    }

    async fn direct_image_deploy(
        state: &AppState,
        app: App,
        deployment: Deployment,
        params: DeployVersionParams,
        guard: crate::DeploymentFlowGuard,
        user_id: String,
    ) -> ApiResult<super::DeployResponseBody> {
        let DeployVersionParams {
            vcpus,
            memory_mib,
            disk_mib,
            env,
            image,
            hypervisor,
        } = params;
        let image_tag = image.expect("direct_image_deploy requires an image");
        let deployment_id = deployment.id;
        let inner = Self::deploy_to_scheduler(
            state,
            &app,
            &deployment,
            DeployParams {
                image_tag: image_tag.clone(),
                vcpus,
                memory_mib,
                disk_mib,
                port: app.port,
                env,
                hypervisor,
            },
        )
        .await?;

        Self::run_zero_downtime_flow(
            state.clone(),
            app,
            deployment,
            inner.clone(),
            user_id,
            true,
            guard,
        );

        Ok(super::DeployResponseBody {
            job_id: Some(inner.job_id),
            deployment_id: Some(deployment_id.to_string()),
            status: "HEALTH_CHECKING".to_string(),
            host_id: Some(inner.host_id),
            vm_id: Some(inner.vm_id),
            image_tag: Some(image_tag),
            message: "Deployment triggered, health check in progress".to_string(),
        })
    }

    pub async fn deploy_app_version(
        state: &AppState,
        auth_user_id: &str,
        app_name: &str,
        params: DeployVersionParams,
    ) -> ApiResult<super::DeployResponseBody> {
        let app = Self::get_app_by_name_and_auth(state, app_name, auth_user_id).await?;
        let git_metadata = Self::fetch_app_git_metadata(state, &app).await;
        let deployment = state
            .app_repo
            .create_deployment(crate::domain::NewDeployment::from_handler(
                app.id,
                auth_user_id.to_string(),
                params.vcpus,
                params.memory_mib,
                params.disk_mib as i64,
                app.port,
                params.env.clone(),
                "manual".to_string(),
                git_metadata.as_ref(),
                params.hypervisor,
            ))
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        let guard = state.try_start_flow(app.id.into()).ok_or_else(|| {
            ApiError::BadRequest(
                "A deployment flow is already in progress for this application".into(),
            )
        })?;

        if let Some(image_tag) = params.image.clone() {
            return Self::direct_image_deploy(
                state,
                app,
                deployment,
                DeployVersionParams {
                    image: Some(image_tag),
                    ..params
                },
                guard,
                auth_user_id.to_string(),
            )
            .await;
        }

        Self::trigger_build(
            state,
            &app,
            &deployment,
            TriggerBuildParams {
                vcpus: params.vcpus,
                memory_mib: params.memory_mib.value() as u64,
                disk_mib: params.disk_mib as u64,
                env: params.env,
                hypervisor: params.hypervisor,
                guard,
            },
        )
        .await?;

        Ok(super::DeployResponseBody {
            job_id: None,
            deployment_id: Some(deployment.id.to_string()),
            status: "BUILDING".to_string(),
            host_id: None,
            vm_id: None,
            image_tag: None,
            message: "Build initiated via NATS".to_string(),
        })
    }

    pub async fn trigger_app_build(
        state: &AppState,
        app: &App,
        git_metadata: Option<&crate::domain::GitMetadata>,
    ) -> ApiResult<Uuid> {
        let vcpus = CpuCores::new(super::DEFAULT_DEPLOYMENT_VCPUS).unwrap();
        let memory_mib = MemoryMb::new(super::DEFAULT_DEPLOYMENT_MEMORY_MIB).unwrap();
        let disk_mib = 1024;
        let env_vars = HashMap::new();

        let deployment = state
            .app_repo
            .create_deployment(crate::domain::NewDeployment::from_handler(
                app.id,
                app.user_id.to_string(),
                vcpus,
                memory_mib,
                disk_mib as i64,
                app.port,
                env_vars.clone(),
                "github_webhook".to_string(),
                git_metadata,
                0,
            ))
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        let guard = state.try_start_flow(app.id.into()).ok_or_else(|| {
            ApiError::BadRequest(
                "A deployment flow is already in progress for this application".into(),
            )
        })?;

        Self::trigger_build(
            state,
            app,
            &deployment,
            TriggerBuildParams {
                vcpus,
                memory_mib: memory_mib.value() as u64,
                disk_mib: disk_mib as u64,
                env: env_vars,
                hypervisor: 0,
                guard,
            },
        )
        .await?;

        Ok(deployment.id)
    }

    pub async fn delete_app(state: &AppState, app: &App) -> ApiResult<()> {
        if let Some(hostname) = &app.hostname {
            state.remove_route(hostname).await.map_err(|e| {
                ApiError::Internal(format!("Failed to remove route for app in router: {}", e))
            })?;
        }

        // Delete from DB first; the physical cleanup runs in the background.
        state
            .app_repo
            .delete_app(app.id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        let cleanup_state = state.clone();
        let app_id = app.id.to_string();
        let user_id = app.user_id.to_string();
        tokio::spawn(async move {
            if let Err(e) = cleanup_state
                .scheduler
                .delete_all_by_app(app_id.clone(), user_id.clone())
                .await
            {
                tracing::error!(
                    app_id = %app_id,
                    error = %e,
                    "Failed to clean up scheduler resources in background"
                );
            }
        });

        state.publish_workspace_event(WorkspaceEvent {
            kind: WorkspaceEventKind::AppDeleted,
            user_id: Some(app.user_id),
            app_id: Some(app.id),
            app_name: Some(app.name.clone()),
            deployment_id: app.active_deployment_id,
            volume_id: None,
            resource_id: None,
        });

        Ok(())
    }

    pub async fn scale_app(state: &AppState, app: &App, params: ScaleAppParams) -> ApiResult<()> {
        let user_uuid = app.user_id;
        let user = state
            .user_repo
            .find_by_id(user_uuid)
            .await?
            .ok_or_else(|| ApiError::NotFound("User not found".into()))?;

        // Force scale-to-zero by default (min_replicas = 0)
        let min = 0;
        let desired = params.desired_replicas.unwrap_or(app.desired_replicas);
        let max = params.max_replicas.unwrap_or(app.max_replicas);

        if desired > 3 || max > 3 {
            return Err(ApiError::BadRequest(
                "Maximum number of replicas is 3".to_string(),
            ));
        }

        if max < 1 {
            return Err(ApiError::BadRequest(
                "Maximum replicas must be at least 1".to_string(),
            ));
        }

        if desired > max {
            return Err(ApiError::BadRequest(
                "Desired replicas cannot be greater than maximum replicas".to_string(),
            ));
        }

        state
            .app_repo
            .update_app_scaling_config(
                app.id,
                desired,
                min, // Forced to 0
                max,
                params
                    .autoscaling_enabled
                    .unwrap_or(app.autoscaling_enabled),
                Some(params.cpu_threshold.unwrap_or(app.cpu_threshold)),
                Some(params.mem_threshold.unwrap_or(app.mem_threshold)),
            )
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        // 2. Fetch updated app state to notify scheduler
        let updated_app = state
            .app_repo
            .get_app(app.id)
            .await?
            .ok_or_else(|| ApiError::Internal("App disappeared after update".into()))?;

        // 3. Notify Scheduler
        // Case A: Manual scaling (if autoscaling is disabled or we just disabled it)
        if !updated_app.autoscaling_enabled {
            state
                .scheduler
                .scale_app(
                    updated_app.id.to_string(),
                    updated_app.desired_replicas as u32,
                    updated_app.user_id.to_string(),
                )
                .await?;
        }

        // Case B: Update autoscaling config in scheduler cache
        state
            .scheduler
            .update_app_scaling_config(mikrom_proto::scheduler::UpdateAppScalingConfigRequest {
                app_id: updated_app.id.to_string(),
                user_id: updated_app.user_id.to_string(),
                min_replicas: updated_app.min_replicas as u32,
                max_replicas: updated_app.max_replicas as u32,
                autoscaling_enabled: updated_app.autoscaling_enabled,
                cpu_threshold: updated_app.cpu_threshold,
                mem_threshold: updated_app.mem_threshold,
                vpc_ipv6_prefix: user.vpc_ipv6_prefix.clone().unwrap_or_default(),
                desired_replicas: updated_app.desired_replicas as u32,
                hostname: updated_app.hostname.clone().unwrap_or_default(),
                last_router_traffic_at: chrono::Utc::now().timestamp(),
                last_scaled_to_zero_at: 0,
            })
            .await?;

        Ok(())
    }

    pub async fn trigger_build(
        state: &AppState,
        app: &App,
        deployment: &Deployment,
        params: TriggerBuildParams,
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
            volume_id: None,
            resource_id: Some(deployment.id.to_string()),
        });

        let mut git_auth_token = None;
        if let (Some(installation_id), Some(app_id), Some(private_key)) = (
            app.github_installation_id,
            &state.github_app_id,
            &state.github_private_key,
        ) {
            match crate::infrastructure::github::get_installation_token(
                app_id,
                private_key,
                installation_id,
            )
            .await
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

        let build_resp: mikrom_proto::builder::BuildResponse = match state
            .nats
            .with_timeout(std::time::Duration::from_secs(5))
            .request("mikrom.builder.build", build_req)
            .await
        {
            Ok(resp) => resp,
            Err(err) => {
                let _ = state
                    .app_repo
                    .update_deployment(
                        deployment.id,
                        UpdateDeploymentParams {
                            status: Some("FAILED".to_string()),
                            ..Default::default()
                        },
                    )
                    .await;
                return Err(ApiError::Internal(format!(
                    "Failed to trigger build via NATS: {}",
                    err
                )));
            },
        };

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
            vcpus: params.vcpus,
            memory_mib: MemoryMb::new(
                u32::try_from(params.memory_mib).unwrap_or(super::DEFAULT_DEPLOYMENT_MEMORY_MIB),
            )
            .unwrap(),
            disk_mib: params.disk_mib as u32,
            port: app.port,
            env: params.env,
            hypervisor: params.hypervisor,
        };

        start_build_polling(state.clone(), task, Some(params.guard)).await;

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
                vcpus: params.vcpus.value(),
                memory_mib: params.memory_mib.value(),
                disk_mib: params.disk_mib,
                port: params.port.value(),
                env: params.env,
                health_check_path: app.health_check_path.clone(),
                hypervisor: params.hypervisor,
                volumes: volumes
                    .into_iter()
                    .map(|v| mikrom_proto::scheduler::Volume {
                        volume_id: v.volume.id.to_string(),
                        size_mib: v.volume.size_mib as u64,
                        read_only: VolumeAccessMode::from_i32(v.access_mode)
                            .is_some_and(|mode| mode.is_read_only()),
                        pool_name: v.volume.pool_name,
                        mount_point: v.mount_point,
                        access_mode: v.access_mode,
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
                let error_text = e.to_string();
                let scheduler_unavailable = error_text.contains("no responders");

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
                    volume_id: None,
                    resource_id: Some(deployment.id.to_string()),
                });
                if scheduler_unavailable {
                    return Err(ApiError::Scheduler(
                        "Scheduler is not available right now".to_string(),
                    ));
                }

                return Err(e);
            },
        };

        let db_status = crate::infrastructure::scheduler::status_name(inner.status);
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
            volume_id: None,
            resource_id: Some(deployment.id.to_string()),
        });

        Ok(inner)
    }

    pub async fn pause_deployment(
        state: &AppState,
        app: &App,
        deployment: &Deployment,
        user_id: String,
    ) -> ApiResult<bool> {
        let job_id = deployment
            .job_id
            .clone()
            .ok_or_else(|| ApiError::BadRequest("Deployment is missing a job id".into()))?;

        tracing::info!(
            app = %app.name,
            job_id = %job_id,
            user_id = %user_id,
            origin = "manual_pause",
            "Forwarding pause request to scheduler"
        );

        let success = state.scheduler.pause_app(job_id.clone(), user_id).await?;

        if success {
            tracing::info!(
                app = %app.name,
                job_id = %job_id,
                origin = "manual_pause",
                "Scheduler pause completed"
            );
            // Update database status
            let _ = state
                .app_repo
                .update_deployment(
                    deployment.id,
                    UpdateDeploymentParams {
                        status: Some("PAUSED".to_string()),
                        job_id: Some(job_id.clone()),
                        image_tag: deployment.image_tag.clone(),
                        build_id: deployment.build_id.clone(),
                        git_commit_hash: deployment.git_commit_hash.clone(),
                        git_commit_message: deployment.git_commit_message.clone(),
                        git_branch: deployment.git_branch.clone(),
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
                volume_id: None,
                resource_id: Some(job_id),
            });
        }

        Ok(success)
    }

    pub async fn resume_deployment(
        state: &AppState,
        app: &App,
        deployment: &Deployment,
        user_id: String,
    ) -> ApiResult<bool> {
        let job_id = deployment
            .job_id
            .clone()
            .ok_or_else(|| ApiError::BadRequest("Deployment is missing a job id".into()))?;

        let success = state.scheduler.resume_app(job_id.clone(), user_id).await?;

        if success {
            // Update database status
            let _ = state
                .app_repo
                .update_deployment(
                    deployment.id,
                    UpdateDeploymentParams {
                        status: Some("RUNNING".to_string()),
                        job_id: Some(job_id.clone()),
                        image_tag: deployment.image_tag.clone(),
                        build_id: deployment.build_id.clone(),
                        git_commit_hash: deployment.git_commit_hash.clone(),
                        git_commit_message: deployment.git_commit_message.clone(),
                        git_branch: deployment.git_branch.clone(),
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
                volume_id: None,
                resource_id: Some(job_id),
            });
        }

        Ok(success)
    }

    pub async fn stop_deployment(
        state: &AppState,
        app: &App,
        deployment: &Deployment,
        user_id: String,
    ) -> ApiResult<(bool, String)> {
        let job_id = deployment
            .job_id
            .clone()
            .ok_or_else(|| ApiError::BadRequest("Deployment is missing a job id".into()))?;

        use mikrom_proto::scheduler::{CancelRequest, CancelResponse};

        let nats_req = CancelRequest {
            job_id: job_id.clone(),
            user_id,
        };

        let inner: CancelResponse = state
            .nats
            .request("mikrom.scheduler.cancel_app", nats_req)
            .await
            .map_err(|e| ApiError::Internal(format!("NATS request failed: {}", e)))?;

        if inner.success {
            // Update database status
            let _ = state
                .app_repo
                .update_deployment(
                    deployment.id,
                    UpdateDeploymentParams {
                        status: Some("STOPPED".to_string()),
                        job_id: Some(job_id.clone()),
                        image_tag: deployment.image_tag.clone(),
                        build_id: deployment.build_id.clone(),
                        git_commit_hash: deployment.git_commit_hash.clone(),
                        git_commit_message: deployment.git_commit_message.clone(),
                        git_branch: deployment.git_branch.clone(),
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
                volume_id: None,
                resource_id: Some(job_id),
            });
        }

        Ok((inner.success, inner.message))
    }

    pub async fn delete_deployment_record(
        state: &AppState,
        app: &App,
        job_id: String,
    ) -> ApiResult<()> {
        state
            .app_repo
            .delete_deployment_by_job_id(&job_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        state.deployment_events.send(app.id).ok();
        state.publish_workspace_event(WorkspaceEvent {
            kind: WorkspaceEventKind::DeploymentChanged,
            user_id: Some(app.user_id),
            app_id: Some(app.id),
            app_name: Some(app.name.clone()),
            deployment_id: None,
            volume_id: None,
            resource_id: Some(job_id),
        });

        Ok(())
    }

    pub fn run_zero_downtime_flow(
        state: crate::AppState,
        app: App,
        deployment: Deployment,
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
    use crate::domain::MockAppRepository;
    use crate::domain::MockVolumeRepository;
    use crate::domain::user::{MockUserRepository, User, UserRole};
    use crate::domain::{App, Deployment};
    use crate::nats::{NatsClient, TypedNatsClient};
    use async_trait::async_trait;
    use sqlx::types::Uuid;
    use std::sync::Arc;

    struct NoRespondersNatsClient;

    #[async_trait]
    impl NatsClient for NoRespondersNatsClient {
        async fn request_raw(
            &self,
            _subject: String,
            _payload: Vec<u8>,
        ) -> anyhow::Result<Vec<u8>> {
            Err(anyhow::anyhow!(
                "NATS request failed: no responders: no responders"
            ))
        }

        async fn publish_raw(&self, _subject: String, _payload: Vec<u8>) -> anyhow::Result<()> {
            Ok(())
        }

        async fn subscribe_raw(&self, _subject: String) -> anyhow::Result<async_nats::Subscriber> {
            Err(anyhow::anyhow!("not used"))
        }
    }

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

    #[tokio::test]
    async fn deploy_to_scheduler_maps_no_responders_to_scheduler_unavailable() {
        let app_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let deployment_id = Uuid::new_v4();

        let mut app_repo = MockAppRepository::new();
        app_repo
            .expect_update_deployment()
            .times(2)
            .returning(|_, _| Ok(()));

        let mut user_repo = MockUserRepository::new();
        user_repo.expect_find_by_id().returning(move |_| {
            Ok(Some(User {
                id: user_id,
                email: "test@example.com".to_string(),
                password_hash: "hash".to_string(),
                role: UserRole::User,
                first_name: None,
                last_name: None,
                vpc_ipv6_prefix: Some("fd00::".to_string()),
            }))
        });

        let mut volume_repo = MockVolumeRepository::new();
        volume_repo
            .expect_list_volumes_by_app()
            .times(1)
            .returning(|_| Ok(vec![]));

        let state = crate::AppState {
            ctx: crate::application::ApiContext::default(),
            user_repo: Arc::new(user_repo),
            app_repo: Arc::new(app_repo),
            github_repo: Arc::new(crate::domain::github::MockGithubRepository::default()),
            volume_repo: Arc::new(volume_repo),
            scheduler: Arc::new(crate::domain::MockScheduler::new()),
            nats: TypedNatsClient::new_custom(Arc::new(NoRespondersNatsClient)),
            router_addr: "http://localhost:8080".to_string(),
            frontend_url: "http://localhost:3000".to_string(),
            api_db: sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap(),
            jwt_secret: "secret".into(),
            master_key: "key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            workspace_events: tokio::sync::broadcast::channel(1).0,
            mesh_status:
                tokio::sync::watch::channel(crate::application::vms::MeshStatus::default()).0,
            acme_email: "admin@example.com".into(),
            acme_staging: true,
            acme_check_interval: 3600,
            github_app_id: None,
            github_private_key: None,
            github_app_slug: None,
            github_webhook_url_base: None,
            active_deployment_flows: Arc::new(dashmap::DashSet::new()),
        };

        let app = App {
            id: app_id,
            user_id,
            name: "demo".into(),
            git_url: "https://example.com/demo".into(),
            port: Port::new(8080).unwrap(),
            hostname: Some("demo.example.com".into()),
            ..Default::default()
        };

        let deployment = Deployment {
            id: deployment_id,
            app_id,
            user_id,
            status: "PENDING".into(),
            vcpus: CpuCores::new(1).unwrap(),
            memory_mib: MemoryMb::new(128).unwrap(),
            port: Port::new(8080).unwrap(),
            ..Default::default()
        };

        let err = DeploymentService::deploy_to_scheduler(
            &state,
            &app,
            &deployment,
            DeployParams {
                image_tag: "image:tag".into(),
                vcpus: CpuCores::new(1).unwrap(),
                memory_mib: MemoryMb::new(256).unwrap(),
                disk_mib: 512,
                port: Port::new(8080).unwrap(),
                env: std::collections::HashMap::new(),
                hypervisor: deployment.hypervisor,
            },
        )
        .await
        .expect_err("no responders should map to scheduler unavailable");

        assert!(matches!(err, ApiError::Scheduler(_)));
    }
}
