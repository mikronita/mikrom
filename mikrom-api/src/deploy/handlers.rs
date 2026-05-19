use crate::AppState;
use crate::auth::AuthUser;
use crate::deploy::orchestrator::DeploymentOrchestrator;
use crate::deploy::service::{DeployParams, DeploymentService};
use crate::error::{ApiError, ApiResult};
use crate::models::app::Deployment;
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
use std::convert::Infallible;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;
use tracing::info;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Deserialize, Serialize, ToSchema, Default, Clone)]
pub struct CreateAppRequest {
    pub name: String,
    pub git_url: String,
    pub port: Option<u32>,
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

#[derive(Debug, Serialize, ToSchema)]
pub struct AppSecretResponse {
    pub github_webhook_secret: Option<String>,
}

#[utoipa::path(
    get,
    path = "/v1/apps/{app_name}/secret",
    params(
        ("app_name" = String, Path, description = "Application name")
    ),
    responses(
        (status = 200, description = "Get application secret", body = AppSecretResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Application not found", body = crate::error::ErrorResponse)
    ),
    tag = "apps",
    security(
        ("jwt" = [])
    )
)]
pub async fn get_app_secret_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(app_name): Path<String>,
) -> ApiResult<Json<AppSecretResponse>> {
    let app = get_app_by_name_and_auth(&state, &app_name, &auth).await?;

    Ok(Json(AppSecretResponse {
        github_webhook_secret: app.github_webhook_secret,
    }))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AppResponse {
    pub id: Uuid,
    pub name: String,
    pub git_url: String,
    pub port: u32,
    pub hostname: Option<String>,
    pub github_webhook_secret: Option<String>,
    pub github_installation_id: Option<i64>,
    pub github_repo_id: Option<i64>,
    pub github_repo_full_name: Option<String>,
    pub active_deployment_id: Option<Uuid>,
    pub health_check_path: String,
    pub drain_timeout: i32,
    pub desired_replicas: i32,
    pub min_replicas: i32,
    pub max_replicas: i32,
    pub autoscaling_enabled: bool,
    pub cpu_threshold: f64,
    pub mem_threshold: f64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ScaleAppRequest {
    pub desired_replicas: Option<i32>,
    pub min_replicas: Option<i32>,
    pub max_replicas: Option<i32>,
    pub autoscaling_enabled: Option<bool>,
    pub cpu_threshold: Option<f64>,
    pub mem_threshold: Option<f64>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ManualDeployRequest {
    pub vcpus: Option<u32>,
    pub memory_mib: Option<u32>,
    pub disk_mib: Option<u32>,
    pub env: Option<std::collections::HashMap<String, String>>,
    pub image: Option<String>,
}

#[utoipa::path(
    post,
    path = "/v1/apps",
    request_body = CreateAppRequest,
    responses(
        (status = 201, description = "Application created", body = AppResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 500, description = "Internal error", body = crate::error::ErrorResponse)
    ),
    tag = "apps",
    security(
        ("jwt" = [])
    )
)]
pub async fn create_app_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<CreateAppRequest>,
) -> ApiResult<(StatusCode, Json<AppResponse>)> {
    let port = payload.port.unwrap_or(8080);

    // Validate replicas limit
    if payload.desired_replicas.unwrap_or(0) > 3
        || payload.max_replicas.unwrap_or(0) > 3
        || payload.min_replicas.unwrap_or(0) > 3
    {
        return Err(ApiError::BadRequest(
            "Maximum number of replicas is 3".to_string(),
        ));
    }
    let hostname = format!(
        "{}.apps.mikrom.spluca.org",
        payload.name.to_lowercase().replace(' ', "-")
    );

    let user_id =
        Uuid::parse_str(&auth.user_id).map_err(|_| ApiError::Auth("Invalid user ID".into()))?;
    let user = state
        .user_repo
        .find_by_id(user_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("User not found".into()))?;

    let webhook_secret = Alphanumeric.sample_string(&mut rand::rng(), 32);

    let app = state
        .app_repo
        .create_app(crate::repositories::app_repository::CreateAppParams {
            name: payload.name.clone(),
            git_url: payload.git_url,
            port: port as i32,
            hostname: Some(hostname),
            user_id,
            github_webhook_secret: Some(webhook_secret.clone()),
            github_installation_id: payload.github_installation_id,
            github_repo_id: payload.github_repo_id,
            github_repo_full_name: payload.github_repo_full_name.clone(),
            health_check_path: payload.health_check_path,
            drain_timeout: payload.drain_timeout,
            ..Default::default()
        })
        .await?;

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
    let _ = state
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
        })
        .await;

    // Automatically create GitHub Webhook if repo is connected
    tracing::info!(
        app_name = %app.name,
        has_inst = app.github_installation_id.is_some(),
        has_repo = app.github_repo_full_name.is_some(),
        has_app_id = state.github_app_id.is_some(),
        has_key = state.github_private_key.is_some(),
        "Checking if automatic GitHub webhook should be created"
    );

    #[allow(clippy::collapsible_if)]
    if let Some(installation_id) = app.github_installation_id {
        if let Some(repo_full_name) = &app.github_repo_full_name {
            if let Some(github_app_id) = &state.github_app_id {
                if let Some(github_private_key) = &state.github_private_key {
                    tracing::info!(app_name = %app.name, "Initiating automatic GitHub webhook creation");
                    let webhook_url = if let Some(base) = &state.github_webhook_url_base {
                        if base.contains("smee.io") {
                            // Smee.io doesn't support subpaths on the public URL
                            base.to_string()
                        } else {
                            format!(
                                "{}/v1/webhooks/github/{}",
                                base.trim_end_matches('/'),
                                app.name
                            )
                        }
                    } else {
                        // Fallback: Try to guess the API URL from the frontend URL
                        let url = format!(
                            "{}/v1/webhooks/github/{}",
                            state.frontend_url.replace("3000", "5001"),
                            app.name
                        );
                        tracing::warn!(
                            app_name = %app.name,
                            %url,
                            "GITHUB_WEBHOOK_URL_BASE is missing, using fragile fallback URL guessing"
                        );
                        url
                    };

                    // Use a background task to not block the response
                    let github_app_id = github_app_id.clone();
                    let github_private_key = github_private_key.clone();
                    let repo_full_name = repo_full_name.clone();
                    let webhook_secret = webhook_secret.clone();

                    let app_id = app.id;
                    tokio::spawn(async move {
                        if let Err(e) = crate::github::create_repository_webhook(
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
                            tracing::info!(app_name = %payload.name, "Successfully created automatic GitHub webhook");
                        }
                    });
                }
            }
        }
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

    Ok((
        StatusCode::CREATED,
        Json(AppResponse {
            id: app.id,
            name: app.name,
            git_url: app.git_url,
            port: app.port as u32,
            hostname: app.hostname,
            github_webhook_secret: app.github_webhook_secret,
            github_installation_id: app.github_installation_id,
            github_repo_id: app.github_repo_id,
            github_repo_full_name: app.github_repo_full_name,
            active_deployment_id: app.active_deployment_id,
            health_check_path: app.health_check_path,
            drain_timeout: app.drain_timeout,
            desired_replicas: app.desired_replicas,
            min_replicas: app.min_replicas,
            max_replicas: app.max_replicas,
            autoscaling_enabled: app.autoscaling_enabled,
            cpu_threshold: app.cpu_threshold,
            mem_threshold: app.mem_threshold,
            created_at: app.created_at,
        }),
    ))
}

#[utoipa::path(
    get,
    path = "/v1/apps",
    responses(
        (status = 200, description = "List applications", body = [AppResponse]),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "apps",
    security(
        ("jwt" = [])
    )
)]
pub async fn list_apps_handler(
    auth: AuthUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<AppResponse>>> {
    let user_id =
        uuid::Uuid::parse_str(&auth.user_id).map_err(|e| ApiError::Internal(e.to_string()))?;
    let apps = state
        .app_repo
        .list_apps_by_user(Some(user_id))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(Json(
        apps.into_iter()
            .map(|a| AppResponse {
                id: a.id,
                name: a.name,
                git_url: a.git_url,
                port: a.port as u32,
                hostname: a.hostname,
                github_webhook_secret: a
                    .github_webhook_secret
                    .as_ref()
                    .map(|_| "********".to_string()),
                github_installation_id: a.github_installation_id,
                github_repo_id: a.github_repo_id,
                github_repo_full_name: a.github_repo_full_name,
                active_deployment_id: a.active_deployment_id,
                health_check_path: a.health_check_path,
                drain_timeout: a.drain_timeout,
                desired_replicas: a.desired_replicas,
                min_replicas: a.min_replicas,
                max_replicas: a.max_replicas,
                autoscaling_enabled: a.autoscaling_enabled,
                cpu_threshold: a.cpu_threshold,
                mem_threshold: a.mem_threshold,
                created_at: a.created_at,
            })
            .collect(),
    ))
}

#[utoipa::path(
    delete,
    path = "/v1/apps/{app_name}",
    params(
        ("app_name" = String, Path, description = "Application name")
    ),
    responses(
        (status = 204, description = "Application deleted"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Application not found", body = crate::error::ErrorResponse)
    ),
    tag = "apps",
    security(
        ("jwt" = [])
    )
)]
pub async fn delete_app_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(app_name): Path<String>,
) -> ApiResult<StatusCode> {
    let app = get_app_by_name_and_auth(&state, &app_name, &auth).await?;

    // Collect volume cleanup targets before the app record is removed.
    let mut cleanup_targets = Vec::new();
    let volumes = state
        .volume_repo
        .list_volumes_by_app(app.id)
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to list volumes for cleanup: {}", e)))?;

    for volume in volumes {
        let snapshots = state
            .volume_repo
            .list_snapshots_by_volume(volume.id)
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to list snapshots for cleanup: {}", e))
            })?;

        cleanup_targets.push((
            volume.id.to_string(),
            volume.pool_name,
            snapshots
                .into_iter()
                .map(|snapshot| snapshot.name)
                .collect::<Vec<_>>(),
        ));
    }

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

        for (volume_id, pool_name, snapshots) in cleanup_targets {
            for snapshot_name in snapshots {
                use mikrom_proto::scheduler::{DeleteSnapshotRequest, DeleteSnapshotResponse};
                let nats_req = DeleteSnapshotRequest {
                    volume_id: volume_id.clone(),
                    snapshot_name,
                    pool_name: pool_name.clone(),
                    host_id: String::new(),
                };

                let _: Result<DeleteSnapshotResponse, _> = cleanup_state
                    .nats
                    .with_timeout(std::time::Duration::from_secs(10))
                    .request("mikrom.scheduler.delete_snapshot", nats_req)
                    .await;
            }

            use mikrom_proto::scheduler::{DeleteVolumeRequest, DeleteVolumeResponse};
            let nats_req = DeleteVolumeRequest {
                volume_id,
                pool_name,
                host_id: String::new(),
            };

            let _: Result<DeleteVolumeResponse, _> = cleanup_state
                .nats
                .with_timeout(std::time::Duration::from_secs(10))
                .request("mikrom.scheduler.delete_volume", nats_req)
                .await;
        }
    });

    state.publish_workspace_event(WorkspaceEvent {
        kind: WorkspaceEventKind::AppDeleted,
        user_id: Some(app.user_id),
        app_id: Some(app.id),
        app_name: Some(app.name),
        deployment_id: app.active_deployment_id,
        volume_id: None,
        resource_id: None,
    });
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    patch,
    path = "/v1/apps/{app_name}/scale",
    request_body = ScaleAppRequest,
    params(
        ("app_name" = String, Path, description = "Application name")
    ),
    responses(
        (status = 200, description = "Scaling configuration updated"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Application not found", body = crate::error::ErrorResponse)
    ),
    tag = "apps",
    security(
        ("jwt" = [])
    )
)]
pub async fn scale_app_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(app_name): Path<String>,
    Json(payload): Json<ScaleAppRequest>,
) -> ApiResult<StatusCode> {
    let app = get_app_by_name_and_auth(&state, &app_name, &auth).await?;

    let user_uuid =
        Uuid::parse_str(&auth.user_id).map_err(|_| ApiError::Auth("Invalid user ID".into()))?;
    let user = state
        .user_repo
        .find_by_id(user_uuid)
        .await?
        .ok_or_else(|| ApiError::NotFound("User not found".into()))?;

    // Validate replicas limit
    if payload.desired_replicas.unwrap_or(0) > 3
        || payload.max_replicas.unwrap_or(0) > 3
        || payload.min_replicas.unwrap_or(0) > 3
    {
        return Err(ApiError::BadRequest(
            "Maximum number of replicas is 3".to_string(),
        ));
    }

    // 1. Update DB (partial updates supported)
    if let Some(replicas) = payload.desired_replicas {
        state
            .app_repo
            .update_app_scaling(app.id, replicas)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
    }

    if payload.autoscaling_enabled.is_some()
        || payload.min_replicas.is_some()
        || payload.max_replicas.is_some()
        || payload.cpu_threshold.is_some()
        || payload.mem_threshold.is_some()
    {
        state
            .app_repo
            .update_app_autoscaling(
                app.id,
                payload.min_replicas.unwrap_or(app.min_replicas),
                payload.max_replicas.unwrap_or(app.max_replicas),
                payload
                    .autoscaling_enabled
                    .unwrap_or(app.autoscaling_enabled),
                payload.cpu_threshold,
                payload.mem_threshold,
            )
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
    }

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
            .await
            .map_err(ApiError::Scheduler)?;
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
        })
        .await
        .map_err(ApiError::Scheduler)?;

    Ok(StatusCode::OK)
}

#[utoipa::path(
    get,
    path = "/v1/apps/{app_name}/deployments",
    params(
        ("app_name" = String, Path, description = "Application name")
    ),
    responses(
        (status = 200, description = "List deployments", body = [Deployment]),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
pub async fn list_deployments_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(app_name): Path<String>,
) -> ApiResult<Json<Vec<Deployment>>> {
    let app = get_app_by_name_and_auth(&state, &app_name, &auth).await?;
    let deployments = state
        .app_repo
        .list_deployments_by_app(app.id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(Json(deployments))
}

#[utoipa::path(
    get,
    path = "/v1/apps/{app_name}/deployments/stream",
    params(
        ("app_name" = String, Path, description = "Application name")
    ),
    responses(
        (status = 200, description = "SSE stream for deployment updates", content_type = "text/event-stream"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
pub async fn deployments_stream_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(app_name): Path<String>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let app = get_app_by_name_and_auth(&state, &app_name, &auth).await?;
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
            .and_then(|deps| serde_json::to_string(&deps).map_err(|e| anyhow::anyhow!(e))) {
                yield Ok(Event::default().data(json));
        }

        loop {
            tokio::select! {
                // 1. Local events (DB changes)
                res = local_stream.next() => {
                    match res {
                        Some(Ok(id)) if id == app_id => {
                            if let Ok(json) = state_clone.app_repo.list_deployments_by_app(app_id).await
                                .and_then(|deps| serde_json::to_string(&deps).map_err(|e| anyhow::anyhow!(e))) {
                                    yield Ok(Event::default().data(json));
                            }
                        },
                        Some(Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(_))) => {
                            // If we lag, just refresh anyway
                            if let Ok(json) = state_clone.app_repo.list_deployments_by_app(app_id).await
                                .and_then(|deps| serde_json::to_string(&deps).map_err(|e| anyhow::anyhow!(e))) {
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
                        .and_then(|deps| serde_json::to_string(&deps).map_err(|e| anyhow::anyhow!(e))) {
                            yield Ok(Event::default().data(json));
                    }
                },
                else => break,
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(5))
            .text("keep-alive"),
    ))
}

#[utoipa::path(
    post,
    path = "/v1/apps/{app_name}/deployments/{deployment_id}/activate",
    params(
        ("app_name" = String, Path, description = "Application name"),
        ("deployment_id" = Uuid, Path, description = "Deployment ID")
    ),
    responses(
        (status = 200, description = "Deployment activated"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Deployment not found", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
pub async fn activate_deployment_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path((app_name, deployment_id)): Path<(String, Uuid)>,
) -> ApiResult<StatusCode> {
    let app = get_app_by_name_and_auth(&state, &app_name, &auth).await?;

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
            user_id: auth.user_id.clone(),
        };

        match state
            .nats
            .request::<_, AppStatusResponse>("mikrom.scheduler.get_job", nats_req)
            .await
        {
            Ok(inner) => Some(crate::scheduler::status_name(inner.status).to_string()),
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
                .await
                .map_err(ApiError::Scheduler)?;

            if !resume_ok {
                return Err(ApiError::BadRequest("Failed to resume deployment".into()));
            }

            let inner = DeployResponse {
                job_id,
                status: DeployStatus::Running as i32,
                host_id: String::new(),
                vm_id: String::new(),
                message: "Resumed".to_string(),
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

#[utoipa::path(
    post,
    path = "/v1/apps/{app_name}/deploy",
    params(
        ("app_name" = String, Path, description = "Application name")
    ),
    request_body = ManualDeployRequest,
    responses(
        (status = 200, description = "Deployment triggered", body = crate::deploy::DeployResponseBody),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Application not found", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
pub async fn deploy_app_version_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(app_name): Path<String>,
    Json(payload): Json<ManualDeployRequest>,
) -> ApiResult<Json<crate::deploy::DeployResponseBody>> {
    let app = get_app_by_name_and_auth(&state, &app_name, &auth).await?;

    let vcpus = payload.vcpus.unwrap_or(1);
    let memory_mib = payload.memory_mib.unwrap_or(256);
    let disk_mib = payload.disk_mib.unwrap_or(1024);
    let env_vars = payload.env.clone().unwrap_or_default();
    let image = payload.image.clone();

    // Try to fetch latest git metadata if linked to GitHub
    let mut git_metadata = None;
    #[allow(clippy::collapsible_if)]
    if let Some(installation_id) = app.github_installation_id {
        if let Some(repo_full_name) = &app.github_repo_full_name {
            match (&state.github_app_id, &state.github_private_key) {
                (Some(github_app_id), Some(github_private_key)) => {
                    // TODO: Fetch the repository's default branch or use a configured branch
                    // instead of hardcoding main/master.
                    // For now, we try main then master as a sensible default.
                    match crate::github::get_repo_latest_commit(
                        github_app_id,
                        github_private_key,
                        installation_id,
                        repo_full_name,
                        "main",
                    )
                    .await
                    {
                        Ok(meta) => git_metadata = Some(meta),
                        Err(_) => {
                            // Try master if main fails
                            if let Ok(meta) = crate::github::get_repo_latest_commit(
                                github_app_id,
                                github_private_key,
                                installation_id,
                                repo_full_name,
                                "master",
                            )
                            .await
                            {
                                git_metadata = Some(meta);
                            }
                        },
                    }
                },
                _ => {
                    tracing::warn!(
                        app_id = %app.id,
                        "GitHub linked but API credentials missing in state"
                    )
                },
            }
        }
    }

    let deployment = state
        .app_repo
        .create_deployment(crate::repositories::app_repository::NewDeployment {
            app_id: app.id,
            user_id: auth.user_id.clone(),
            vcpus: vcpus as i32,
            memory_mib: memory_mib as i64,
            disk_mib: disk_mib as i64,
            port: app.port,
            env_vars: env_vars.clone(),
            trigger_source: "manual".to_string(),
            git_commit_hash: git_metadata
                .as_ref()
                .and_then(|m| m.git_commit_hash.clone()),
            git_commit_message: git_metadata
                .as_ref()
                .and_then(|m| m.git_commit_message.clone()),
            git_branch: git_metadata.as_ref().and_then(|m| m.git_branch.clone()),
        })
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    // Protect against concurrent flows
    let guard = state.try_start_flow(app.id.into()).ok_or_else(|| {
        ApiError::BadRequest("A deployment flow is already in progress for this application".into())
    })?;

    // If an image is provided directly, skip the build phase and deploy immediately
    if let Some(image_tag) = image {
        info!(app = %app.name, image = %image_tag, "Direct image deployment requested, skipping build");

        // 1. Trigger deployment
        let inner = match DeploymentService::deploy_to_scheduler(
            &state,
            &app,
            &deployment,
            DeployParams {
                image_tag: image_tag.clone(),
                vcpus,
                memory_mib,
                disk_mib,
                port: app.port as u32,
                env: env_vars.clone(),
            },
        )
        .await
        {
            Ok(inner) => inner,
            Err(e) => {
                return Err(e);
            },
        };

        // 2. Start Zero-Downtime orchestration in background
        DeploymentService::run_zero_downtime_flow(
            state.clone(),
            app,
            deployment.clone(),
            inner.clone(),
            auth.user_id.clone(),
            true,
            guard,
        );

        return Ok(Json(crate::deploy::DeployResponseBody {
            job_id: Some(inner.job_id),
            deployment_id: Some(deployment.id.to_string()),
            status: "HEALTH_CHECKING".to_string(),
            host_id: Some(inner.host_id),
            vm_id: Some(inner.vm_id),
            image_tag: Some(image_tag),
            message: "Deployment triggered, health check in progress".to_string(),
        }));
    }

    // Default: Trigger build
    DeploymentService::trigger_build(
        &state,
        &app,
        &deployment,
        vcpus,
        memory_mib as u64,
        disk_mib as u64,
        env_vars,
        guard,
    )
    .await?;

    Ok(Json(crate::deploy::DeployResponseBody {
        job_id: None,
        deployment_id: Some(deployment.id.to_string()),
        status: "BUILDING".to_string(),
        host_id: None,
        vm_id: None,
        image_tag: None,
        message: "Build initiated via NATS".to_string(),
    }))
}

pub async fn trigger_app_build(
    state: crate::AppState,
    app: crate::models::app::App,
    git_metadata: Option<crate::repositories::app_repository::GitMetadata>,
) -> ApiResult<Uuid> {
    let vcpus = 1;
    let memory_mib = 256;
    let disk_mib = 1024;
    let env_vars = std::collections::HashMap::new();

    let deployment = state
        .app_repo
        .create_deployment(crate::repositories::app_repository::NewDeployment {
            app_id: app.id,
            user_id: app.user_id.to_string(),
            vcpus,
            memory_mib: memory_mib as i64,
            disk_mib: disk_mib as i64,
            port: app.port,
            env_vars: env_vars.clone(),
            trigger_source: "github_webhook".to_string(),
            git_commit_hash: git_metadata
                .as_ref()
                .and_then(|m| m.git_commit_hash.clone()),
            git_commit_message: git_metadata
                .as_ref()
                .and_then(|m| m.git_commit_message.clone()),
            git_branch: git_metadata.as_ref().and_then(|m| m.git_branch.clone()),
        })
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    // Protect against concurrent flows
    let guard = state.try_start_flow(app.id.into()).ok_or_else(|| {
        ApiError::BadRequest("A deployment flow is already in progress for this application".into())
    })?;

    DeploymentService::trigger_build(
        &state,
        &app,
        &deployment,
        vcpus as u32,
        memory_mib as u64,
        disk_mib as u64,
        env_vars,
        guard,
    )
    .await?;

    Ok(deployment.id)
}

async fn get_app_by_name_and_auth(
    state: &AppState,
    app_name: &str,
    auth: &AuthUser,
) -> ApiResult<crate::models::app::App> {
    let app = state
        .app_repo
        .get_app_by_name(app_name)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or_else(|| ApiError::NotFound("Application not found".into()))?;

    if app.user_id.to_string() != auth.user_id {
        return Err(ApiError::Forbidden);
    }

    Ok(app)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppState;
    use crate::auth::AuthUser;
    use crate::repositories::app_repository::MockAppRepository;
    use crate::repositories::user_repository::{MockUserRepository, User, UserRole};
    use crate::scheduler::MockScheduler;
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
        let github_repo =
            Arc::new(crate::repositories::github_repository::MockGithubRepository::default());
        let volume_repo =
            Arc::new(crate::repositories::volume_repository::MockVolumeRepository::new());
        let scheduler = Arc::new(MockScheduler::new());

        AppState {
            user_repo,
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
            mesh_status: tokio::sync::watch::channel(crate::vms::MeshStatus::default()).0,
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
    async fn test_scale_app_validation_limit() {
        let mut state = create_test_state().await;

        let user_id = Uuid::new_v4();
        let auth = AuthUser {
            user_id: user_id.to_string(),
            email: "test@example.com".to_string(),
            role: UserRole::User,
        };

        let app = crate::models::app::App {
            id: Uuid::new_v4(),
            user_id,
            name: "test-app".to_string(),
            ..crate::models::app::App::default()
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

        let result = scale_app_handler(
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
}
