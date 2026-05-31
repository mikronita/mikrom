use crate::AppState;
use crate::application::deployment::{
    AppResponse, DeployRequestPayload, DeployResponseBody, DeploymentOrchestrator,
    DeploymentService, build_app_response, build_app_response_with_scale_state,
    resolve_app_scale_state_from_running_count, resolve_deployment_hypervisor,
    resolve_deployment_memory_mib, resolve_deployment_vcpus,
};
use crate::domain::CreateAppParams;
use crate::domain::Deployment;
use crate::domain::types::{CpuCores, MemoryMb, Port};
use crate::error::{ApiError, ApiResult};
use crate::infrastructure::auth::extractor::{AuthUser, TenantContext};
use crate::workspace::{WorkspaceEvent, WorkspaceEventKind};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::sse::{Event, Sse},
};
use futures::stream::Stream;
use rand::distr::{Alphanumeric, SampleString};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;
use tracing::info;
use uuid::Uuid;

#[derive(Debug, Deserialize, Serialize, rovo::schemars::JsonSchema, Default, Clone)]
pub struct CreateAppRequest {
    pub name: String,
    pub git_url: String,
    pub port: Option<Port>,
    pub github_installation_id: Option<i64>,
    pub github_repo_id: Option<i64>,
    pub github_repo_full_name: Option<String>,
    pub health_check_path: Option<String>,
    pub drain_timeout: Option<i32>,
    pub desired_replicas: Option<i32>,
    pub min_replicas: Option<i32>,
    pub max_replicas: Option<i32>,
    pub autoscaling_enabled: Option<bool>,
}

#[derive(Debug, Serialize, rovo::schemars::JsonSchema)]
pub struct AppSecretResponse {
    pub github_webhook_secret: Option<String>,
}

#[rovo::rovo]
pub async fn get_app_handler(
    tenant_ctx: TenantContext,
    State(state): State<AppState>,
    Path(app_name): Path<String>,
) -> ApiResult<Json<AppResponse>> {
    let app = DeploymentService::get_app_by_name_and_auth(
        &state,
        &app_name,
        &tenant_ctx.tenant.id.to_string(),
    )
    .await?;
    Ok(Json(build_app_response(&state, &app).await))
}

#[rovo::rovo]
pub async fn get_app_secret_handler(
    tenant_ctx: TenantContext,
    State(state): State<AppState>,
    Path(app_name): Path<String>,
) -> ApiResult<Json<AppSecretResponse>> {
    let app = DeploymentService::get_app_by_name_and_auth(
        &state,
        &app_name,
        &tenant_ctx.tenant.id.to_string(),
    )
    .await?;

    Ok(Json(AppSecretResponse {
        github_webhook_secret: app.github_webhook_secret,
    }))
}

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct ScaleAppRequest {
    pub desired_replicas: Option<i32>,
    pub min_replicas: Option<i32>,
    pub max_replicas: Option<i32>,
    pub autoscaling_enabled: Option<bool>,
    pub cpu_threshold: Option<f64>,
    pub mem_threshold: Option<f64>,
}

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct ManualDeployRequest {
    /// CPU cores to allocate. Allowed values: 1, 2, 3, or 4.
    pub vcpus: Option<CpuCores>,
    /// Memory to allocate in MiB. Allowed values: 512, 1024, 2048, or 4096.
    pub memory_mib: Option<MemoryMb>,
    pub disk_mib: Option<u32>,
    pub env: Option<std::collections::HashMap<String, String>>,
    pub image: Option<String>,
    /// Hypervisor to use: "firecracker" or "cloud-hypervisor". Defaults to scheduler-selected.
    pub hypervisor: Option<String>,
}

#[rovo::rovo]
pub async fn create_app_handler(
    auth: AuthUser,
    tenant_ctx: TenantContext,
    State(state): State<AppState>,
    Json(payload): Json<CreateAppRequest>,
) -> ApiResult<(StatusCode, Json<AppResponse>)> {
    let port = payload.port.unwrap_or_else(|| Port::new(8080).unwrap());
    let user_id = Uuid::parse_str(&auth.user_id).map_err(|e| ApiError::Internal(e.to_string()))?;

    // Force scale-to-zero by default (min_replicas = 0)
    let min = 0;
    let desired = payload.desired_replicas.unwrap_or(1);
    let max = payload.max_replicas.unwrap_or(1);

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
    let hostname = format!(
        "{}.apps.mikrom.spluca.org",
        payload.name.to_lowercase().replace(' ', "-")
    );

    let tenant_id = tenant_ctx.tenant.id;
    let webhook_secret = Alphanumeric.sample_string(&mut rand::rng(), 32);

    if payload.github_installation_id.is_some()
        && payload.github_repo_full_name.is_some()
        && state.github_webhook_url_base.is_none()
    {
        return Err(ApiError::BadRequest(
            "GITHUB_WEBHOOK_URL_BASE must be configured to create GitHub webhooks".into(),
        ));
    }

    let app = DeploymentService::create_app(
        &state,
        CreateAppParams {
            name: payload.name.clone(),
            git_url: payload.git_url,
            port,
            hostname: Some(hostname),
            user_id,
            tenant_id,
            github_webhook_secret: Some(webhook_secret.clone()),
            github_installation_id: payload.github_installation_id,
            github_repo_id: payload.github_repo_id,
            github_repo_full_name: payload.github_repo_full_name.clone(),
            health_check_path: payload.health_check_path,
            drain_timeout: payload.drain_timeout,
            desired_replicas: Some(desired),
            min_replicas: Some(min),
            max_replicas: Some(max),
            autoscaling_enabled: payload.autoscaling_enabled,
            ..Default::default()
        },
    )
    .await?;

    DeploymentService::maybe_create_github_webhook(&state, &app, &webhook_secret).await?;

    Ok((
        StatusCode::CREATED,
        Json(build_app_response(&state, &app).await),
    ))
}

#[rovo::rovo]
pub async fn list_apps_handler(
    tenant_ctx: TenantContext,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<AppResponse>>> {
    let tenant_id = tenant_ctx.tenant.id;
    let apps = state
        .app_repo
        .list_apps_by_tenant(Some(tenant_id))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let now = chrono::Utc::now().timestamp();

    let running_counts = match state
        .scheduler
        .list_apps(mikrom_proto::scheduler::ListAppsRequest {
            tenant_id: tenant_id.to_string(),
            status: Some(mikrom_proto::scheduler::DeployStatus::Running as i32),
        })
        .await
    {
        Ok(resp) => {
            let mut counts = HashMap::new();
            for job in resp.apps {
                if let Ok(app_id) = Uuid::parse_str(&job.app_id) {
                    *counts.entry(app_id).or_insert(0) += 1;
                }
            }
            Some(counts)
        },
        Err(err) => {
            tracing::warn!(
                error = %err,
                "Failed to list running jobs from scheduler while listing apps"
            );
            None
        },
    };

    let mut responses = Vec::with_capacity(apps.len());
    for app in apps {
        let mut response = if let Some(running_counts) = &running_counts {
            let running_count = *running_counts.get(&app.id).unwrap_or(&0);
            build_app_response_with_scale_state(
                &app,
                resolve_app_scale_state_from_running_count(&app, running_count, now),
            )
        } else {
            build_app_response(&state, &app).await
        };
        if response.github_webhook_secret.is_some() {
            response.github_webhook_secret = Some("********".to_string());
        }
        responses.push(response);
    }

    Ok(Json(responses))
}

#[rovo::rovo]
pub async fn delete_app_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(app_name): Path<String>,
) -> ApiResult<StatusCode> {
    let app = DeploymentService::get_app_by_name_and_auth(&state, &app_name, &auth.user_id).await?;

    #[allow(clippy::collapsible_if)]
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
    let tenant_id = app.tenant_id.to_string();
    tokio::spawn(async move {
        if let Err(e) = cleanup_state
            .scheduler
            .delete_all_by_app(app_id.clone(), tenant_id.clone())
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
        tenant_id: Some(app.tenant_id),
        user_id: None,
        app_id: Some(app.id),
        app_name: Some(app.name),
        deployment_id: app.active_deployment_id,
        volume_id: None,
        resource_id: None,
    });
    Ok(StatusCode::NO_CONTENT)
}

#[rovo::rovo]
pub async fn scale_app_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(app_name): Path<String>,
    Json(payload): Json<ScaleAppRequest>,
) -> ApiResult<StatusCode> {
    let app = DeploymentService::get_app_by_name_and_auth(&state, &app_name, &auth.user_id).await?;

    let user_uuid =
        Uuid::parse_str(&auth.user_id).map_err(|_| ApiError::Auth("Invalid user ID".into()))?;
    let user = state
        .user_repo
        .find_by_id(user_uuid)
        .await?
        .ok_or_else(|| ApiError::NotFound("User not found".into()))?;

    // Force scale-to-zero by default (min_replicas = 0)
    let min = 0;
    let desired = payload.desired_replicas.unwrap_or(app.desired_replicas);
    let max = payload.max_replicas.unwrap_or(app.max_replicas);

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
            max, // payload.max_replicas.unwrap_or(app.max_replicas)
            payload
                .autoscaling_enabled
                .unwrap_or(app.autoscaling_enabled),
            Some(payload.cpu_threshold.unwrap_or(app.cpu_threshold)),
            Some(payload.mem_threshold.unwrap_or(app.mem_threshold)),
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
                updated_app.tenant_id.to_string(),
            )
            .await?;
    }

    // Case B: Update autoscaling config in scheduler cache
    state
        .scheduler
        .update_app_scaling_config(mikrom_proto::scheduler::UpdateAppScalingConfigRequest {
            app_id: updated_app.id.to_string(),
            tenant_id: updated_app.tenant_id.to_string(),
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

    Ok(StatusCode::OK)
}

#[rovo::rovo]
pub async fn list_deployments_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(app_name): Path<String>,
) -> ApiResult<Json<Vec<Deployment>>> {
    let app = DeploymentService::get_app_by_name_and_auth(&state, &app_name, &auth.user_id).await?;
    let deployments = state
        .app_repo
        .list_deployments_by_app(app.id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(Json(deployments))
}

#[rovo::rovo]
pub async fn deployments_stream_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(app_name): Path<String>,
) -> ApiResult<crate::error::SseResponse<impl Stream<Item = Result<Event, Infallible>>>> {
    let app = DeploymentService::get_app_by_name_and_auth(&state, &app_name, &auth.user_id).await?;
    let app_id = app.id;
    let rx = state.deployment_events.subscribe();
    let state_clone = state.clone();

    // Subscribe to NATS for instant cluster-wide updates
    let nats_sub = state
        .nats
        .subscribe("mikrom.scheduler.job_updates")
        .await
        .map_err(|e| ApiError::Internal(format!("NATS sub error: {}", e)))?;

    let stream = async_stream::stream! {
        let mut local_stream = BroadcastStream::new(rx);
        let mut nats_stream = nats_sub;
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));

        // 0. Initial yield: send current state immediately upon connection
        if let Ok(json) = state_clone.app_repo.list_deployments_by_app(app_id).await
            .and_then(|deps| serde_json::to_string(&deps).map_err(|e| crate::domain::DomainError::Infrastructure(e.to_string()))) {
                yield Ok(Event::default().data(json));
        }

        loop {
            tokio::select! {
                // 1. Local events (DB changes)
                res = local_stream.next() => {
                    match res {
                        Some(Ok(id)) if id == app_id => {
                            if let Ok(json) = state_clone.app_repo.list_deployments_by_app(app_id).await
                                .and_then(|deps| serde_json::to_string(&deps).map_err(|e| crate::domain::DomainError::Infrastructure(e.to_string()))) {
                                    yield Ok(Event::default().data(json));
                            }
                        },
                        Some(Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(_))) => {
                            // If we lag, just refresh anyway
                            if let Ok(json) = state_clone.app_repo.list_deployments_by_app(app_id).await
                                .and_then(|deps| serde_json::to_string(&deps).map_err(|e| crate::domain::DomainError::Infrastructure(e.to_string()))) {
                                    yield Ok(Event::default().data(json));
                            }
                        },
                        _ => {}
                    }
                },
                // 2. Cluster events (Scheduler changes)
                Some(msg) = nats_stream.next() => {
                    use prost::Message;
                    use mikrom_proto::scheduler::AppInfo;
                    if let Ok(json) = async {
                        let info = AppInfo::decode(&msg.payload[..])?;
                        if info.app_id != app_id.to_string() { return Err(anyhow::anyhow!("Mismatch")); }
                        let deps = state_clone.app_repo.list_deployments_by_app(app_id).await?;
                        serde_json::to_string(&deps).map_err(|e| anyhow::anyhow!(e))
                    }.await {
                        yield Ok(Event::default().data(json));
                    }
                },
                // 3. Periodic refresh (Brute force fallback to ensure UI stays in sync)
                _ = interval.tick() => {
                    if let Ok(json) = state_clone.app_repo.list_deployments_by_app(app_id).await
                        .and_then(|deps| serde_json::to_string(&deps).map_err(|e| crate::domain::DomainError::Infrastructure(e.to_string()))) {
                            yield Ok(Event::default().data(json));
                    }
                },
                else => break,
            }
        }
    };

    Ok(crate::error::SseResponse(
        Sse::new(stream).keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(std::time::Duration::from_secs(5))
                .text("keep-alive"),
        ),
    ))
}

#[rovo::rovo]
pub async fn activate_deployment_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path((app_name, deployment_id)): Path<(String, Uuid)>,
) -> ApiResult<StatusCode> {
    let app = DeploymentService::get_app_by_name_and_auth(&state, &app_name, &auth.user_id).await?;

    let deployment = state
        .app_repo
        .get_deployment(deployment_id)
        .await?
        .ok_or(ApiError::NotFound("Deployment not found".into()))?;

    if deployment.app_id != app.id {
        return Err(ApiError::BadRequest(
            "Deployment does not belong to this application".into(),
        ));
    }

    let runtime_status = if let Some(job_id) = deployment.job_id.clone() {
        use mikrom_proto::scheduler::{AppStatusRequest, AppStatusResponse};

        let nats_req = AppStatusRequest {
            job_id,
            tenant_id: auth.user_id.clone(),
        };

        match state
            .nats
            .request::<_, AppStatusResponse>("mikrom.scheduler.get_job", nats_req)
            .await
        {
            Ok(inner) => {
                Some(crate::infrastructure::scheduler::status_name(inner.status).to_string())
            },
            Err(e) => {
                tracing::warn!(
                    app_id = %app.id,
                    deployment_id = %deployment.id,
                    error = %e,
                    "Failed to resolve runtime deployment status from scheduler, falling back to DB status"
                );
                None
            },
        }
    } else {
        None
    };

    let current_status = runtime_status
        .as_deref()
        .unwrap_or(deployment.status.as_str());

    if current_status == "FAILED" {
        return Err(ApiError::BadRequest(
            "Cannot activate a failed deployment".into(),
        ));
    }

    match deployment.job_id.clone() {
        Some(job_id) => {
            info!(
                job_id = %job_id,
                status = %current_status,
                db_status = %deployment.status,
                "Activating deployment with zero-downtime flow..."
            );

            use mikrom_proto::scheduler::{DeployResponse, DeployStatus};

            if current_status == "RUNNING" {
                info!(
                    app = %app.name,
                    deployment_id = %deployment.id,
                    job_id = %job_id,
                    "Promoting running deployment immediately"
                );

                let (updated_app, previous_active_id) =
                    DeploymentOrchestrator::promote_deployment_to_active(
                        &state,
                        app,
                        deployment.id,
                    )
                    .await
                    .map_err(|e| ApiError::Internal(e.to_string()))?;

                if let Some(old_active_id) = previous_active_id.filter(|id| *id != deployment.id) {
                    DeploymentOrchestrator::drain_previous_deployment_after_promotion(
                        &state,
                        &updated_app.name,
                        Some(old_active_id),
                    )
                    .await
                    .map_err(|e| ApiError::Internal(e.to_string()))?;
                }

                return Ok(StatusCode::OK);
            }

            if current_status != "PAUSED" && current_status != "STOPPED" {
                return Err(ApiError::BadRequest(format!(
                    "Deployment is not ready to promote yet (current status: {})",
                    current_status
                )));
            }

            // Protect against concurrent flows before any scheduler interaction
            let guard = state.try_start_flow(app.id.into()).ok_or_else(|| {
                ApiError::BadRequest(
                    "A deployment flow is already in progress for this application".into(),
                )
            })?;

            let job_id = deployment.job_id.clone().ok_or_else(|| {
                ApiError::BadRequest("Paused deployment is missing a job id".into())
            })?;

            info!(app = %app.name, "Deployment is paused or stopped, resuming it first...");

            let resume_ok = state
                .scheduler
                .resume_app(job_id.clone(), "system".to_string())
                .await?;

            if !resume_ok {
                return Err(ApiError::BadRequest("Failed to resume deployment".into()));
            }

            let inner = DeployResponse {
                job_id,
                status: DeployStatus::Running as i32,
                host_id: String::new(),
                vm_id: String::new(),
                message: "Resumed".to_string(),
                hypervisor: deployment.hypervisor,
            };
            let cleanup_on_failure = true;

            DeploymentService::run_zero_downtime_flow(
                state.clone(),
                app,
                deployment,
                inner,
                auth.user_id,
                cleanup_on_failure,
                guard,
            );

            Ok(StatusCode::ACCEPTED)
        },
        None => {
            info!(app = %app.name, deployment_id = %deployment.id, "Activating deployment record only...");

            let _ =
                DeploymentOrchestrator::promote_deployment_to_active(&state, app, deployment_id)
                    .await
                    .map_err(|e| ApiError::Internal(e.to_string()))?;

            Ok(StatusCode::OK)
        },
    }
}

#[rovo::rovo]
pub async fn deploy_app(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<DeployRequestPayload>,
) -> ApiResult<Json<DeployResponseBody>> {
    let vcpus = resolve_deployment_vcpus(payload.vcpus)?;
    let memory_mib = resolve_deployment_memory_mib(payload.memory_mib)?;
    let disk_mib = payload.disk_mib.unwrap_or(1024);
    let env_vars = payload.env.clone().unwrap_or_default();
    let image = payload.image.clone();
    let hypervisor = resolve_deployment_hypervisor(payload.hypervisor.as_deref());

    let app = match state.app_repo.get_app_by_name(&payload.app_name).await? {
        Some(app) => {
            if app.tenant_id.to_string() != auth.user_id {
                return Err(ApiError::Forbidden);
            }
            app
        },
        None => {
            // Auto-create app if git_url is provided
            if let Some(git_url) = payload.git_url {
                let user_uuid = uuid::Uuid::parse_str(&auth.user_id)
                    .map_err(|e| ApiError::Internal(e.to_string()))?;

                let create_params = crate::domain::CreateAppParams {
                    user_id: user_uuid,
                    tenant_id: user_uuid,
                    name: payload.app_name.clone(),
                    git_url,
                    port: payload.port.unwrap_or_else(|| Port::new(8080).unwrap()),
                    ..Default::default()
                };
                DeploymentService::create_app(&state, create_params).await?
            } else {
                return Err(ApiError::NotFound(
                    "Application not found and no git_url provided for auto-creation".into(),
                ));
            }
        },
    };

    let deployment = state
        .app_repo
        .create_deployment(crate::domain::NewDeployment::from_handler(
            app.id,
            app.tenant_id.to_string(),
            vcpus,
            memory_mib,
            disk_mib as i64,
            app.port,
            env_vars.clone(),
            "api_deploy".to_string(),
            None, // No git metadata for direct deploy
            hypervisor,
        ))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let guard = state.try_start_flow(app.id.into()).ok_or_else(|| {
        ApiError::BadRequest("A deployment flow is already in progress for this application".into())
    })?;

    let inner = DeploymentService::deploy_to_scheduler(
        &state,
        &app,
        &deployment,
        crate::application::deployment::DeployParams {
            image_tag: image.clone(),
            vcpus,
            memory_mib,
            disk_mib,
            port: app.port,
            env: env_vars,
            hypervisor,
        },
    )
    .await?;

    DeploymentService::run_zero_downtime_flow(
        state.clone(),
        app,
        deployment.clone(),
        inner.clone(),
        auth.user_id,
        true,
        guard,
    );

    Ok(Json(DeployResponseBody {
        job_id: Some(inner.job_id),
        deployment_id: Some(deployment.id.to_string()),
        status: "HEALTH_CHECKING".to_string(),
        host_id: Some(inner.host_id),
        vm_id: Some(inner.vm_id),
        image_tag: Some(image),
        message: "Deployment triggered, health check in progress".to_string(),
    }))
}

#[rovo::rovo]
pub async fn deploy_app_version_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(app_name): Path<String>,
    Json(payload): Json<ManualDeployRequest>,
) -> ApiResult<Json<DeployResponseBody>> {
    let vcpus = resolve_deployment_vcpus(payload.vcpus)?;
    let memory_mib = resolve_deployment_memory_mib(payload.memory_mib)?;
    let disk_mib = payload.disk_mib.unwrap_or(1024);
    let env_vars = payload.env.clone().unwrap_or_default();
    let image = payload.image.clone();
    let hypervisor = resolve_deployment_hypervisor(payload.hypervisor.as_deref());

    let response = DeploymentService::deploy_app_version(
        &state,
        &auth.user_id,
        &app_name,
        crate::application::deployment::service::DeployVersionParams {
            vcpus,
            memory_mib,
            disk_mib,
            env: env_vars,
            image,
            hypervisor,
        },
    )
    .await?;

    Ok(Json(response))
}

pub async fn trigger_app_build(
    state: crate::AppState,
    app: crate::domain::App,
    git_metadata: Option<crate::domain::GitMetadata>,
) -> ApiResult<Uuid> {
    DeploymentService::trigger_app_build(&state, &app, git_metadata.as_ref()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppState;
    use crate::application::deployment::{AppScaleState, resolve_app_scale_state};
    use crate::auth::AuthUser;
    use crate::domain::MockDatabaseRepository;
    use crate::domain::MockScheduler;
    use crate::domain::github::MockGithubRepository;
    use crate::domain::{
        MockAppRepository, MockUserRepository, MockVolumeRepository, User, UserRole,
    };
    use axum::extract::{Path, State};
    use std::sync::Arc;
    use uuid::Uuid;

    struct MockNats;
    #[async_trait::async_trait]
    impl crate::nats::NatsClient for MockNats {
        async fn request_raw(&self, _s: String, _p: Vec<u8>) -> anyhow::Result<Vec<u8>> {
            Ok(vec![])
        }
        async fn publish_raw(&self, _s: String, _p: Vec<u8>) -> anyhow::Result<()> {
            Ok(())
        }
        async fn subscribe_raw(&self, _s: String) -> anyhow::Result<async_nats::Subscriber> {
            Err(anyhow::anyhow!("Mock subscriber not implemented"))
        }
    }

    async fn create_test_state() -> AppState {
        let user_repo = Arc::new(MockUserRepository::new());
        let app_repo = Arc::new(MockAppRepository::new());
        let github_repo = Arc::new(MockGithubRepository::default());
        let volume_repo = Arc::new(MockVolumeRepository::new());
        let mut scheduler = MockScheduler::new();
        scheduler
            .expect_list_apps()
            .times(0..)
            .returning(|_| Ok(mikrom_proto::scheduler::ListAppsResponse { apps: vec![] }));
        let scheduler = Arc::new(scheduler);

        AppState {
            ctx: crate::application::ApiContext::default(),
            user_repo,
            tenant_repo: Arc::new(crate::domain::MockTenantRepository::new()),
            database_repo: Arc::new(MockDatabaseRepository::new()),
            app_repo,
            github_repo,
            volume_repo,
            scheduler,
            nats: crate::nats::TypedNatsClient::new_custom(Arc::new(MockNats)),
            router_addr: String::new(),
            frontend_url: String::new(),
            api_db: sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap(),
            jwt_secret: "secret".to_string(),
            master_key: "key".to_string(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            workspace_events: tokio::sync::broadcast::channel(1).0,
            mesh_status:
                tokio::sync::watch::channel(crate::application::vms::MeshStatus::default()).0,
            acme_email: String::new(),
            acme_staging: true,
            acme_check_interval: 0,
            github_app_id: None,
            github_private_key: None,
            github_app_slug: None,
            github_webhook_url_base: None,
            active_deployment_flows: Arc::new(dashmap::DashSet::new()),
        }
    }

    #[tokio::test]
    async fn test_resolve_app_scale_state_variants() {
        let state = create_test_state().await;
        let app = crate::domain::App {
            id: Uuid::new_v4(),
            desired_replicas: 0,
            ..crate::domain::App::default()
        };
        assert!(matches!(
            resolve_app_scale_state(&state, &app).await,
            AppScaleState::ScaledToZero
        ));

        let app = crate::domain::App {
            id: Uuid::new_v4(),
            desired_replicas: 1,
            active_deployment_id: None,
            ..crate::domain::App::default()
        };
        assert!(matches!(
            resolve_app_scale_state(&state, &app).await,
            AppScaleState::ScaledToZero
        ));

        let deployment_id = Uuid::new_v4();
        let app_id = Uuid::new_v4();
        let mut mock_app_repo = MockAppRepository::new();
        mock_app_repo
            .expect_get_active_deployment()
            .returning(move |_| {
                Ok(Some(crate::domain::Deployment {
                    id: deployment_id,
                    app_id,
                    tenant_id: Uuid::new_v4(),
                    build_id: None,
                    image_tag: None,
                    job_id: Some("job-1".to_string()),
                    ipv6_address: Some("fd00::1".to_string()),
                    status: "RUNNING".to_string(),
                    vcpus: crate::domain::types::CpuCores::new(1).unwrap(),
                    memory_mib: crate::domain::types::MemoryMb::new(512).unwrap(),
                    disk_mib: 1024,
                    port: crate::domain::types::Port::new(8080).unwrap(),
                    env_vars: serde_json::json!({}),
                    git_commit_hash: None,
                    git_commit_message: None,
                    git_branch: None,
                    trigger_source: "test".to_string(),
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    hypervisor: 0,
                }))
            });

        let mut state = create_test_state().await;
        state.app_repo = Arc::new(mock_app_repo);

        let mut mock_scheduler = crate::domain::MockScheduler::new();
        let app_id_str = app_id.to_string();
        mock_scheduler.expect_list_apps().returning(move |req| {
            if req.status == Some(mikrom_proto::scheduler::DeployStatus::Running as i32) {
                Ok(mikrom_proto::scheduler::ListAppsResponse {
                    apps: vec![mikrom_proto::scheduler::AppInfo {
                        app_id: app_id_str.clone(),
                        ..Default::default()
                    }],
                })
            } else {
                Ok(mikrom_proto::scheduler::ListAppsResponse::default())
            }
        });
        state.scheduler = Arc::new(mock_scheduler);

        let app = crate::domain::App {
            id: app_id,
            desired_replicas: 1,
            active_deployment_id: Some(deployment_id),
            last_router_traffic_at: chrono::Utc::now().timestamp(),
            ..crate::domain::App::default()
        };
        assert!(matches!(
            resolve_app_scale_state(&state, &app).await,
            AppScaleState::Active
        ));
    }

    #[tokio::test]
    async fn test_scale_app_validation_limit() {
        let mut state = create_test_state().await;

        let user_id = Uuid::new_v4();
        let auth = AuthUser {
            user_id: user_id.to_string(),
            email: "test@example.com".to_string(),
            role: UserRole::User,
        };

        let app = crate::domain::App {
            id: Uuid::new_v4(),
            tenant_id: user_id,
            name: "test-app".to_string(),
            ..crate::domain::App::default()
        };

        // Mock app repo
        let mut mock_app_repo = MockAppRepository::new();
        mock_app_repo
            .expect_get_app_by_name()
            .returning(move |_| Ok(Some(app.clone())));
        state.app_repo = Arc::new(mock_app_repo);

        // Mock user repo
        let mut mock_user_repo = MockUserRepository::new();
        mock_user_repo.expect_find_by_id().returning(move |_| {
            Ok(Some(User {
                id: user_id,
                email: "test@example.com".to_string(),
                password_hash: "hash".to_string(),
                role: UserRole::User,
                first_name: None,
                last_name: None,
                vpc_ipv6_prefix: None,
            }))
        });
        state.user_repo = Arc::new(mock_user_repo);

        // Request too many replicas
        let payload = ScaleAppRequest {
            desired_replicas: Some(4), // LIMIT IS 3
            min_replicas: None,
            max_replicas: None,
            autoscaling_enabled: None,
            cpu_threshold: None,
            mem_threshold: None,
        };

        let result = __scale_app_handler_impl(
            auth,
            State(state),
            Path("test-app".to_string()),
            axum::Json(payload),
        )
        .await;

        match result {
            Err(ApiError::BadRequest(msg)) => {
                assert!(msg.contains("Maximum number of replicas is 3"))
            },
            _ => panic!("Expected BadRequest error, got {:?}", result),
        }
    }

    #[tokio::test]
    async fn test_scale_app_returns_service_unavailable_when_scheduler_is_down() {
        let mut state = create_test_state().await;

        let user_id = Uuid::new_v4();
        let app_id = Uuid::new_v4();
        let auth = AuthUser {
            user_id: user_id.to_string(),
            email: "test@example.com".to_string(),
            role: UserRole::User,
        };

        let shared_app = Arc::new(std::sync::Mutex::new(crate::domain::App {
            id: app_id,
            tenant_id: user_id,
            name: "test-app".to_string(),
            hostname: Some("test-app.apps.mikrom.spluca.org".to_string()),
            desired_replicas: 1,
            min_replicas: 1,
            max_replicas: 3,
            autoscaling_enabled: false,
            ..crate::domain::App::default()
        }));

        let mut mock_app_repo = MockAppRepository::new();
        {
            let shared_app = shared_app.clone();
            mock_app_repo.expect_get_app_by_name().returning(move |_| {
                let app = shared_app.lock().unwrap().clone();
                Ok(Some(app))
            });
        }
        {
            let shared_app = shared_app.clone();
            mock_app_repo.expect_update_app_scaling_config().returning(
                move |_, desired, min, max, enabled, cpu, mem| {
                    let mut app = shared_app.lock().unwrap();
                    app.desired_replicas = desired;
                    app.min_replicas = min;
                    app.max_replicas = max;
                    app.autoscaling_enabled = enabled;
                    if let Some(cpu) = cpu {
                        app.cpu_threshold = cpu;
                    }
                    if let Some(mem) = mem {
                        app.mem_threshold = mem;
                    }
                    Ok(())
                },
            );
        }
        {
            let shared_app = shared_app.clone();
            mock_app_repo.expect_get_app().returning(move |_| {
                let app = shared_app.lock().unwrap().clone();
                Ok(Some(app))
            });
        }

        state.app_repo = Arc::new(mock_app_repo);

        let mut mock_user_repo = MockUserRepository::new();
        mock_user_repo.expect_find_by_id().returning(move |_| {
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
        state.user_repo = Arc::new(mock_user_repo);

        let mut mock_scheduler = MockScheduler::new();
        mock_scheduler.expect_scale_app().returning(|_, _, _| {
            Err(crate::domain::DomainError::Infrastructure(
                "NATS request failed: no responders: no responders".to_string(),
            ))
        });
        mock_scheduler.expect_update_app_scaling_config().times(0);
        state.scheduler = Arc::new(mock_scheduler);

        let result = __scale_app_handler_impl(
            auth,
            State(state),
            Path("test-app".to_string()),
            axum::Json(ScaleAppRequest {
                desired_replicas: Some(2),
                min_replicas: None,
                max_replicas: None,
                autoscaling_enabled: None,
                cpu_threshold: None,
                mem_threshold: None,
            }),
        )
        .await;

        match result {
            Err(ApiError::Domain(crate::domain::DomainError::Infrastructure(_))) => {},
            _ => panic!("Expected Scheduler/Infrastructure error, got {:?}", result),
        }
    }

    #[tokio::test]
    async fn get_app_handler_returns_app_response() {
        let mut state = create_test_state().await;
        let user_id = Uuid::new_v4();
        let app_id = Uuid::new_v4();

        let app = crate::domain::App {
            id: app_id,
            tenant_id: user_id,
            name: "test-app".to_string(),
            git_url: "https://github.com/test/repo".to_string(),
            port: crate::domain::types::Port::new(8080).unwrap(),
            hostname: Some("test-app.apps.mikrom.spluca.org".to_string()),
            desired_replicas: 1,
            min_replicas: 0,
            max_replicas: 1,
            autoscaling_enabled: false,
            ..crate::domain::App::default()
        };

        let mut mock_app_repo = MockAppRepository::new();
        mock_app_repo
            .expect_get_app_by_name()
            .with(mockall::predicate::eq("test-app"))
            .times(1)
            .returning(move |_| Ok(Some(app.clone())));
        state.app_repo = Arc::new(mock_app_repo);

        let tenant_ctx = crate::infrastructure::auth::extractor::TenantContext {
            tenant: crate::domain::Tenant {
                id: user_id,
                tenant_id: user_id.to_string().chars().take(6).collect(),
                name: "test".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
        };

        let result =
            __get_app_handler_impl(tenant_ctx, State(state), Path("test-app".to_string())).await;

        let response = result.expect("should return app");
        assert_eq!(response.0.name, "test-app");
        assert_eq!(response.0.id, app_id);
    }

    #[tokio::test]
    async fn get_app_handler_returns_not_found_when_missing() {
        let mut state = create_test_state().await;
        let user_id = Uuid::new_v4();

        let mut mock_app_repo = MockAppRepository::new();
        mock_app_repo
            .expect_get_app_by_name()
            .with(mockall::predicate::eq("missing-app"))
            .times(1)
            .returning(|_| Ok(None));
        state.app_repo = Arc::new(mock_app_repo);

        let tenant_ctx = crate::infrastructure::auth::extractor::TenantContext {
            tenant: crate::domain::Tenant {
                id: user_id,
                tenant_id: user_id.to_string().chars().take(6).collect(),
                name: "test".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
        };

        let result =
            __get_app_handler_impl(tenant_ctx, State(state), Path("missing-app".to_string())).await;

        match result {
            Err(ApiError::NotFound(msg)) => assert!(msg.contains("not found")),
            _ => panic!("Expected NotFound error, got {:?}", result),
        }
    }
}
