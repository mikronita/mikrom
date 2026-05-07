use crate::AppState;
use crate::deploy::worker::{BuildTask, start_build_polling};
use crate::error::{ApiError, ApiResult};
use crate::models::app::{App, Deployment};
use crate::repositories::app_repository::UpdateDeploymentParams;
use mikrom_proto::scheduler::{AppConfig, DeployRequest, DeployResponse};

pub struct DeploymentService;

pub struct DeployParams {
    pub image_tag: String,
    pub vcpus: u32,
    pub memory_mib: u32,
    pub disk_mib: u32,
    pub env: std::collections::HashMap<String, String>,
}

impl DeploymentService {
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
                ..Default::default()
            }),
            deployment_id: deployment.id.to_string(),
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
                    ip_address: Some(inner.ip_address.clone()),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        state.deployment_events.send(app.id).ok();

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
        use crate::repositories::app_repository::UpdateDeploymentParams;
        use std::time::Duration;
        use tracing::{debug, error, info};

        let app_id = app.id;

        tokio::spawn(async move {
            // Keep the guard in this task
            let _guard = _guard;

            let result = async {
                // 1. Polling for Health
                let mut healthy = false;
                let max_attempts = 60; // 120 seconds total
                for attempt in 1..=max_attempts {
                    if attempt % 5 == 1 {
                        info!(
                            app = %app.name,
                            job_id = %inner.job_id,
                            attempt = attempt,
                            "Checking health for zero-downtime deployment..."
                        );
                    } else {
                        debug!(
                            app = %app.name,
                            job_id = %inner.job_id,
                            attempt = attempt,
                            "Checking health for zero-downtime deployment..."
                        );
                    }

                    let health_req = mikrom_proto::scheduler::CheckHealthRequest {
                        job_id: inner.job_id.clone(),
                        user_id: user_id.clone(),
                    };

                    match state
                        .nats
                        .with_timeout(Duration::from_secs(7))
                        .request::<_, mikrom_proto::scheduler::CheckHealthResponse>(
                            "mikrom.scheduler.check_health",
                            health_req,
                        )
                        .await
                    {
                        Ok(resp) if resp.is_healthy => {
                            healthy = true;
                            info!(app = %app.name, "New deployment is healthy!");
                            break;
                        },
                        Ok(resp) => {
                            debug!(
                                app = %app.name,
                                message = %resp.message,
                                "Health check returned unhealthy"
                            );
                        },
                        Err(e) => {
                            debug!(
                                app = %app.name,
                                error = %e,
                                "Health check request failed"
                            );
                        },
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }

                if !healthy {
                    if cleanup_on_failure {
                        error!(
                            app = %app.name,
                            "Zero-downtime deployment failed: health check timeout. Cleaning up new VM."
                        );
                        state
                            .scheduler
                            .pause_app(inner.job_id, "system".to_string())
                            .await
                            .map_err(|e| anyhow::anyhow!(e))?;
                        state
                            .app_repo
                            .update_deployment(
                                deployment.id,
                                UpdateDeploymentParams {
                                    status: Some("FAILED".to_string()),
                                    ..Default::default()
                                },
                            )
                            .await?;
                    } else {
                        error!(
                            app = %app.name,
                            "Promotion failed: health check timeout. App remains in preview."
                        );
                    }
                    state.deployment_events.send(app.id).ok();
                    return Ok::<(), anyhow::Error>(());
                }

                // 2. Identify old deployment
                let old_active_id = match state.app_repo.get_app(app.id).await {
                    Ok(Some(a)) => a.active_deployment_id,
                    _ => None,
                };

                // 3. Promote new deployment to active
                info!(
                    app = %app.name,
                    deployment_id = %deployment.id,
                    "Promoting new deployment to active"
                );
                state
                    .app_repo
                    .set_active_deployment(app.id, deployment.id)
                    .await?;

                // 4. Notify router (atomic switch)
                // We fetch the app again to ensure we have the newly active deployment state
                if let Some(app_refreshed) = state.app_repo.get_app(app.id).await? {
                    if app_refreshed.active_deployment_id != Some(deployment.id) {
                        return Err(anyhow::anyhow!("Failed to verify active deployment promotion in DB"));
                    }
                    state.notify_router(&app_refreshed).await?;
                } else {
                    return Err(anyhow::anyhow!("Application not found during promotion"));
                }

                state.deployment_events.send(app.id).ok();

                // 5. Drain Phase
                if let Some(old_id) = old_active_id {
                    // Validate drain_timeout to avoid negative or extreme values
                    let drain_secs = if app.drain_timeout < 0 {
                        0u64
                    } else {
                        app.drain_timeout as u64
                    };

                    if drain_secs > 0 {
                        info!(app = %app.name, "Waiting {}s for drain phase...", drain_secs);
                        tokio::time::sleep(Duration::from_secs(drain_secs)).await;
                    }

                    // 6. Stop old VM
                    if let Some(old_dep) = state.app_repo.get_deployment(old_id).await?
                        && let Some(old_job_id) = old_dep.job_id
                    {
                        info!(app = %app.name, job_id = %old_job_id, "Stopping old version");
                        state
                            .scheduler
                            .pause_app(old_job_id, "system".to_string())
                            .await
                            .map_err(|e| anyhow::anyhow!(e))?;
                        state
                            .app_repo
                            .update_deployment(
                                old_id,
                                UpdateDeploymentParams {
                                    status: Some("STOPPED".to_string()),
                                    ..Default::default()
                                },
                            )
                            .await?;
                    }
                }
                Ok(())
            }
            .await;

            if let Err(e) = result {
                error!(app_id = %app_id, error = %e, "Zero-downtime deployment flow failed unexpectedly");
            }
        });
    }
}
