use crate::error::{ApiError, ApiResult};
use axum::{
    Json,
    extract::{Path, State},
    response::{
        IntoResponse,
        sse::{Event, Sse},
    },
};
use serde::Serialize;
use std::collections::HashMap;
use tokio_stream::StreamExt;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct LiveDeploymentInfo {
    pub job_id: String,
    pub deployment_id: String,
    pub app_id: String,
    pub app_name: String,
    pub image: String,
    pub status: String,
    pub host_id: String,
    pub vm_id: String,
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LiveDeploymentStatus {
    pub job_id: String,
    pub deployment_id: String,
    pub app_id: String,
    pub app_name: String,
    pub image: String,
    pub status: String,
    pub host_id: String,
    pub vm_id: String,
    pub scheduled_at: i64,
    pub started_at: i64,
    pub stopped_at: i64,
    pub error_message: String,
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
    pub vcpus: i32,
    pub memory_mib: i64,
}

#[utoipa::path(
    get,
    path = "/deployments/active",
    responses(
        (status = 200, description = "List of active deployments", body = [LiveDeploymentInfo]),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
#[tracing::instrument(skip(state, auth))]
pub async fn list_active_deployments(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
) -> ApiResult<Json<Vec<LiveDeploymentInfo>>> {
    // 1. Get all deployments for this user from DB
    let deployments = state
        .app_repo
        .list_deployments_by_user(&auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    // 2. Try to get real-time status from scheduler for active ones
    let mut scheduler_apps = HashMap::new();

    use mikrom_proto::scheduler::{ListAppsRequest, ListAppsResponse};
    use prost::Message;

    let nats_req = ListAppsRequest {
        user_id: auth.user_id.clone(),
        status: None,
    };

    let mut buf = Vec::new();
    if let Some(inner) = async {
        if nats_req.encode(&mut buf).is_err() {
            return None;
        }
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            state
                .nats_client
                .request("mikrom.scheduler.list_apps", buf.into()),
        )
        .await
        .ok()?
        .ok()?;
        ListAppsResponse::decode(&response.payload[..]).ok()
    }
    .await
    {
        for app in inner.apps {
            scheduler_apps.insert(app.job_id.clone(), app);
        }
    }

    // 3. Map deployments to LiveDeploymentInfo, using scheduler data if available
    let mut active_deployments = Vec::new();
    for dep in deployments {
        let (status, host_id, vm_id, cpu_usage, ram_used_bytes) =
            if let Some(job_id_real) = &dep.job_id {
                if let Some(sch_app) = scheduler_apps.get(job_id_real) {
                    (
                        crate::scheduler::status_name(sch_app.status).to_string(),
                        sch_app.host_id.clone(),
                        sch_app.vm_id.clone(),
                        sch_app.cpu_usage,
                        sch_app.ram_used_bytes,
                    )
                } else {
                    (dep.status.clone(), String::new(), String::new(), 0.0, 0)
                }
            } else {
                (dep.status.clone(), String::new(), String::new(), 0.0, 0)
            };

        // Get app name from repo
        let app_name = if let Ok(Some(app)) = state.app_repo.get_app(dep.app_id).await {
            app.name
        } else {
            "Unknown".to_string()
        };

        active_deployments.push(LiveDeploymentInfo {
            job_id: dep.job_id.unwrap_or_default(),
            deployment_id: dep.id.to_string(),
            app_id: dep.app_id.to_string(),
            app_name,
            image: dep.image_tag.unwrap_or_default(),
            status,
            host_id,
            vm_id,
            cpu_usage,
            ram_used_bytes,
        });
    }

    Ok(Json(active_deployments))
}

#[utoipa::path(
    get,
    path = "/deployments/events",
    responses(
        (status = 200, description = "SSE stream of active deployment events"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
#[tracing::instrument(skip(state, auth))]
pub async fn watch_deployments(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
) -> ApiResult<impl IntoResponse> {
    let nats_sub = state
        .nats_client
        .subscribe("mikrom.scheduler.job_updates")
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to subscribe to job updates: {}", e)))?;

    let local_rx = state.deployment_events.subscribe();

    let auth_user_id = auth.user_id.clone();
    let state_clone = state.clone();

    // Unified stream combining cluster (NATS) and local (DB) events
    let stream = async_stream::stream! {
        let mut nats_stream = nats_sub;
        let mut local_stream = tokio_stream::wrappers::BroadcastStream::new(local_rx);
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3));

        // 0. Initial yield: send current state of all active deployments for the user
        if let Ok(apps) = state_clone.app_repo.list_apps_by_user(&auth_user_id).await {
            for app in apps {
                if let Ok(deps) = state_clone.app_repo.list_deployments_by_app(app.id).await {
                    for dep in deps {
                        if ["RUNNING", "BUILDING", "SCHEDULED", "STOPPED", "FAILED"].contains(&dep.status.as_str()) {
                            let data = serde_json::json!({
                                "job_id": dep.job_id.clone().unwrap_or_default(),
                                "deployment_id": dep.id.to_string(),
                                "app_id": dep.app_id.to_string(),
                                "app_name": app.name.clone(),
                                "image": dep.image_tag.clone().unwrap_or_default(),
                                "status": dep.status,
                                "host_id": String::new(),
                                "vm_id": String::new(),
                                "cpu_usage": 0.0,
                                "ram_used_bytes": 0,
                                "scheduled_at": 0,
                                "started_at": 0,
                                "stopped_at": 0,
                                "error_message": "",
                            });
                            if let Ok(json) = serde_json::to_string(&data) {
                                yield Ok::<Event, std::convert::Infallible>(Event::default().data(json));
                            }
                        }
                    }
                }
            }
        }

        loop {
            tokio::select! {
                // 1. Cluster-wide events from NATS
                Some(msg) = nats_stream.next() => {
                    use prost::Message;
                    use mikrom_proto::scheduler::AppInfo;
                    if let Some(job) = AppInfo::decode(&msg.payload[..]).ok().filter(|j| j.user_id == auth_user_id) {
                            let data = serde_json::json!({
                                "job_id": job.job_id,
                                "deployment_id": job.deployment_id,
                                "app_id": job.app_id,
                                "app_name": job.app_name,
                                "image": job.image,
                                "status": crate::scheduler::status_name(job.status),
                                "host_id": job.host_id,
                                "vm_id": job.vm_id,
                                "cpu_usage": job.cpu_usage,
                                "ram_used_bytes": job.ram_used_bytes,
                                "scheduled_at": 0,
                                "started_at": 0,
                                "stopped_at": 0,
                                "error_message": "",
                            });
                            if let Ok(json) = serde_json::to_string(&data) {
                                yield Ok::<Event, std::convert::Infallible>(Event::default().data(json));
                            }
                    }
                },
                // 2. Local events from DB
                res = local_stream.next() => {
                    if let Ok(deps) = async {
                        let app_id = res.and_then(|r| r.ok()).ok_or(anyhow::anyhow!("No ID"))?;
                        state_clone.app_repo.list_deployments_by_app(app_id).await
                    }.await {
                        for dep in deps {
                            if ["RUNNING", "BUILDING", "SCHEDULED", "STOPPED", "FAILED"].contains(&dep.status.as_str()) {
                                let data = serde_json::json!({
                                    "job_id": dep.job_id.clone().unwrap_or_default(),
                                    "deployment_id": dep.id.to_string(),
                                    "app_id": dep.app_id.to_string(),
                                    "app_name": "",
                                    "image": dep.image_tag.clone().unwrap_or_default(),
                                    "status": dep.status,
                                    "host_id": String::new(),
                                    "vm_id": String::new(),
                                    "cpu_usage": 0.0,
                                    "ram_used_bytes": 0,
                                    "scheduled_at": 0,
                                    "started_at": 0,
                                    "stopped_at": 0,
                                    "error_message": "",
                                });
                                if let Ok(json) = serde_json::to_string(&data) {
                                    yield Ok::<Event, std::convert::Infallible>(Event::default().data(json));
                                }
                            }
                        }
                    }
                },
                // 3. Periodic refresh (Brute force fallback)
                _ = interval.tick() => {
                    use mikrom_proto::scheduler::{ListAppsRequest, ListAppsResponse};
                    use prost::Message;

                    let nats_req = ListAppsRequest {
                        user_id: auth_user_id.clone(),
                        status: None,
                    };

                    let mut buf = Vec::new();
                    let scheduler_apps = if nats_req.encode(&mut buf).is_ok() {
                        if let Ok(response) = state_clone
                            .nats_client
                            .request("mikrom.scheduler.list_apps", buf.into())
                            .await
                        {
                            ListAppsResponse::decode(&response.payload[..]).ok().map(|r| r.apps).unwrap_or_default()
                        } else {
                            Vec::new()
                        }
                    } else {
                        Vec::new()
                    };

                    if !scheduler_apps.is_empty() {
                        for job in scheduler_apps {
                             let data = serde_json::json!({
                                "job_id": job.job_id,
                                "deployment_id": job.deployment_id,
                                "app_id": job.app_id,
                                "app_name": job.app_name,
                                "image": job.image,
                                "status": crate::scheduler::status_name(job.status),
                                "host_id": job.host_id,
                                "vm_id": job.vm_id,
                                "cpu_usage": job.cpu_usage,
                                "ram_used_bytes": job.ram_used_bytes,
                                "scheduled_at": 0,
                                "started_at": 0,
                                "stopped_at": 0,
                                "error_message": "",
                            });
                            if let Ok(json) = serde_json::to_string(&data) {
                                yield Ok::<Event, std::convert::Infallible>(Event::default().data(json));
                            }
                        }
                    } else {
                        // Fallback to DB if scheduler is unreachable or returns nothing
                        if let Ok(apps) = state_clone.app_repo.list_apps_by_user(&auth_user_id).await {
                            for app in apps {
                                if let Ok(deps) = state_clone.app_repo.list_deployments_by_app(app.id).await {
                                    for dep in deps {
                                        if ["RUNNING", "BUILDING", "SCHEDULED", "STOPPED", "FAILED"].contains(&dep.status.as_str()) {
                                            let data = serde_json::json!({
                                                "job_id": dep.job_id.clone().unwrap_or_default(),
                                                "deployment_id": dep.id.to_string(),
                                                "app_id": dep.app_id.to_string(),
                                                "app_name": app.name.clone(),
                                                "image": dep.image_tag.clone().unwrap_or_default(),
                                                "status": dep.status,
                                                "host_id": String::new(),
                                                "vm_id": String::new(),
                                                "cpu_usage": 0.0,
                                                "ram_used_bytes": 0,
                                                "scheduled_at": 0,
                                                "started_at": 0,
                                                "stopped_at": 0,
                                                "error_message": "",
                                            });
                                            if let Ok(json) = serde_json::to_string(&data) {
                                                yield Ok::<Event, std::convert::Infallible>(Event::default().data(json));
                                            }
                                        }
                                    }
                                }
                            }
                        }
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

pub async fn validate_app_deployment(
    state: &crate::AppState,
    auth: &crate::auth::AuthUser,
    app_name: &str,
    job_id: &str,
) -> ApiResult<(crate::models::app::App, crate::models::app::Deployment)> {
    let app = state
        .app_repo
        .get_app_by_name(app_name)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or(ApiError::NotFound("Application not found".into()))?;

    if app.user_id.to_string() != auth.user_id {
        return Err(ApiError::Forbidden);
    }

    let deployment = if let Some(stripped) = job_id.strip_prefix("temp-") {
        let dep_id = uuid::Uuid::parse_str(stripped)
            .map_err(|_| ApiError::BadRequest("Invalid temp ID".into()))?;
        state
            .app_repo
            .get_deployment(dep_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or(ApiError::NotFound("Deployment not found".into()))?
    } else {
        state
            .app_repo
            .get_deployment_by_job_id(job_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or(ApiError::NotFound("Deployment not found".into()))?
    };

    if deployment.app_id != app.id {
        return Err(ApiError::BadRequest(
            "Deployment does not belong to this application".into(),
        ));
    }

    Ok((app, deployment))
}

#[utoipa::path(
    get,
    path = "/apps/{app_name}/deployments/{job_id}",
    params(
        ("app_name" = String, Path, description = "Application name"),
        ("job_id" = String, Path, description = "Deployment Job ID")
    ),
    responses(
        (status = 200, description = "Get live deployment details", body = LiveDeploymentStatus),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Deployment not found", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
#[tracing::instrument(skip(state), fields(app_name = %app_name, job_id = %job_id))]
pub async fn get_deployment_status(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, job_id)): Path<(String, String)>,
) -> ApiResult<Json<LiveDeploymentStatus>> {
    use mikrom_proto::scheduler::{AppStatusRequest, AppStatusResponse};
    use prost::Message;

    let (app, dep) = validate_app_deployment(&state, &auth, &app_name, &job_id).await?;

    // If it's a temporary ID from BUILDING/SCHEDULED phase
    if job_id.starts_with("temp-") {
        return Ok(Json(LiveDeploymentStatus {
            job_id: job_id.clone(),
            deployment_id: dep.id.to_string(),
            app_id: dep.app_id.to_string(),
            app_name: app.name,
            image: dep.image_tag.unwrap_or_default(),
            status: dep.status,
            host_id: String::new(),
            vm_id: String::new(),
            scheduled_at: 0,
            started_at: 0,
            stopped_at: 0,
            error_message: String::new(),
            cpu_usage: 0.0,
            ram_used_bytes: 0,
            vcpus: dep.vcpus,
            memory_mib: dep.memory_mib,
        }));
    }

    let nats_req = AppStatusRequest {
        job_id: job_id.clone(),
        user_id: auth.user_id.clone(),
    };

    let mut buf = Vec::new();
    nats_req
        .encode(&mut buf)
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let response = state
        .nats_client
        .request("mikrom.scheduler.get_job", buf.into())
        .await
        .map_err(|e| ApiError::Internal(format!("NATS request failed: {}", e)))?;

    let inner = AppStatusResponse::decode(&response.payload[..])
        .map_err(|e| ApiError::Internal(format!("Failed to parse NATS response: {}", e)))?;

    let deployment_status = LiveDeploymentStatus {
        job_id: inner.job_id,
        deployment_id: dep.id.to_string(),
        app_id: dep.app_id.to_string(),
        app_name: app.name,
        image: dep.image_tag.unwrap_or_default(),
        status: crate::scheduler::status_name(inner.status).to_string(),
        host_id: inner.host_id,
        vm_id: inner.vm_id,
        scheduled_at: inner.scheduled_at,
        started_at: inner.started_at,
        stopped_at: inner.stopped_at,
        error_message: inner.error_message,
        cpu_usage: inner.cpu_usage,
        ram_used_bytes: inner.ram_used_bytes,
        vcpus: dep.vcpus,
        memory_mib: dep.memory_mib,
    };

    Ok(Json(deployment_status))
}

#[utoipa::path(
    get,
    path = "/apps/{app_name}/deployments/{job_id}/logs",
    params(
        ("app_name" = String, Path, description = "Application name"),
        ("job_id" = String, Path, description = "Deployment Job ID")
    ),
    responses(
        (status = 200, description = "SSE stream for VM logs", content_type = "text/event-stream"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Deployment not found", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
pub async fn get_deployment_logs(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, job_id)): Path<(String, String)>,
) -> ApiResult<impl IntoResponse> {
    // 1. Validate app ownership and deployment connection
    let _ = validate_app_deployment(&state, &auth, &app_name, &job_id).await?;

    // 2. Get VM ID from scheduler via NATS
    use mikrom_proto::scheduler::AppStatusRequest;
    use prost::Message;

    let nats_req = AppStatusRequest {
        job_id: job_id.clone(),
        user_id: auth.user_id.clone(),
    };

    let mut buf = Vec::new();
    nats_req
        .encode(&mut buf)
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let response = state
        .nats_client
        .request("mikrom.scheduler.get_job", buf.into())
        .await
        .map_err(|e| ApiError::Internal(format!("NATS request failed: {}", e)))?;

    let inner = mikrom_proto::scheduler::AppStatusResponse::decode(&response.payload[..])
        .map_err(|e| ApiError::Internal(format!("Failed to parse NATS response: {}", e)))?;

    let vm_id = inner.vm_id;
    if vm_id.is_empty() {
        return Err(ApiError::BadRequest(
            "VM is not yet active or assigned".to_string(),
        ));
    }

    let subject = format!("mikrom.logs.{}", vm_id);
    let subscription = state
        .nats_client
        .subscribe(subject)
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to subscribe to logs: {}", e)))?;

    let stream = subscription.map(|msg| {
        let text = String::from_utf8_lossy(&msg.payload).to_string();
        Ok::<Event, std::convert::Infallible>(Event::default().data(text))
    });

    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(1))
            .text("keep-alive"),
    ))
}

#[utoipa::path(
    post,
    path = "/apps/{app_name}/deployments/{job_id}/pause",
    params(
        ("app_name" = String, Path, description = "Application name"),
        ("job_id" = String, Path, description = "Deployment Job ID")
    ),
    responses(
        (status = 200, description = "Deployment paused"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Application/Deployment not found", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
#[tracing::instrument(skip(state), fields(app_name = %app_name, job_id = %job_id))]
pub async fn pause_deployment(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, job_id)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    // Validate app ownership and deployment connection
    let (app, deployment) = validate_app_deployment(&state, &auth, &app_name, &job_id).await?;

    let success = state
        .scheduler
        .pause_app(job_id.clone(), auth.user_id.clone())
        .await
        .map_err(ApiError::Scheduler)?;

    if success {
        // Update database status
        let _ = state
            .app_repo
            .update_deployment_status(
                deployment.id,
                "STOPPED",
                Some(job_id),
                deployment.image_tag,
                deployment.build_id,
                None,
                deployment.git_commit_hash,
                deployment.git_commit_message,
                deployment.git_branch,
            )
            .await;
        state.deployment_events.send(app.id).ok();

        Ok(Json(
            serde_json::json!({ "success": true, "message": "Paused" }),
        ))
    } else {
        Err(ApiError::BadRequest("Failed to pause".to_string()))
    }
}

#[utoipa::path(
    post,
    path = "/apps/{app_name}/deployments/{job_id}/resume",
    params(
        ("app_name" = String, Path, description = "Application name"),
        ("job_id" = String, Path, description = "Deployment Job ID")
    ),
    responses(
        (status = 200, description = "Deployment resumed"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Application/Deployment not found", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
#[tracing::instrument(skip(state), fields(app_name = %app_name, job_id = %job_id))]
pub async fn resume_deployment(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, job_id)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    // Validate app ownership and deployment connection
    let (app, deployment) = validate_app_deployment(&state, &auth, &app_name, &job_id).await?;

    let success = state
        .scheduler
        .resume_app(job_id.clone(), auth.user_id.clone())
        .await
        .map_err(ApiError::Scheduler)?;

    if success {
        // Update database status
        let _ = state
            .app_repo
            .update_deployment_status(
                deployment.id,
                "RUNNING",
                Some(job_id),
                deployment.image_tag,
                deployment.build_id,
                None,
                deployment.git_commit_hash,
                deployment.git_commit_message,
                deployment.git_branch,
            )
            .await;
        state.deployment_events.send(app.id).ok();

        Ok(Json(
            serde_json::json!({ "success": true, "message": "Resumed" }),
        ))
    } else {
        Err(ApiError::BadRequest("Failed to resume".to_string()))
    }
}

#[utoipa::path(
    delete,
    path = "/apps/{app_name}/deployments/{job_id}",
    params(
        ("app_name" = String, Path, description = "Application name"),
        ("job_id" = String, Path, description = "Deployment Job ID")
    ),
    responses(
        (status = 200, description = "Deployment stopped"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Application/Deployment not found", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
#[tracing::instrument(skip(state), fields(app_name = %app_name, job_id = %job_id))]
pub async fn stop_deployment(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, job_id)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    // Validate app ownership and deployment connection
    let (app, deployment) = validate_app_deployment(&state, &auth, &app_name, &job_id).await?;

    use mikrom_proto::scheduler::CancelRequest;
    use prost::Message;

    let nats_req = CancelRequest {
        job_id: job_id.clone(),
        user_id: auth.user_id.clone(),
    };

    let mut buf = Vec::new();
    nats_req
        .encode(&mut buf)
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let response = state
        .nats_client
        .request("mikrom.scheduler.cancel_app", buf.into())
        .await
        .map_err(|e| ApiError::Internal(format!("NATS request failed: {}", e)))?;

    let inner = mikrom_proto::scheduler::CancelResponse::decode(&response.payload[..])
        .map_err(|e| ApiError::Internal(format!("Failed to parse NATS response: {}", e)))?;

    if inner.success {
        // Update database status
        let _ = state
            .app_repo
            .update_deployment_status(
                deployment.id,
                "STOPPED",
                Some(job_id),
                deployment.image_tag,
                deployment.build_id,
                None,
                deployment.git_commit_hash,
                deployment.git_commit_message,
                deployment.git_branch,
            )
            .await;

        state.deployment_events.send(app.id).ok();
        Ok(Json(
            serde_json::json!({ "success": true, "message": inner.message }),
        ))
    } else {
        Err(ApiError::NotFound(inner.message))
    }
}

#[utoipa::path(
    delete,
    path = "/apps/{app_name}/deployments/{job_id}/delete",
    params(
        ("app_name" = String, Path, description = "Application name"),
        ("job_id" = String, Path, description = "Deployment Job ID")
    ),
    responses(
        (status = 200, description = "Deployment record deleted"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Application/Deployment not found", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
#[tracing::instrument(skip(state), fields(app_name = %app_name, job_id = %job_id))]
pub async fn delete_deployment_record(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, job_id)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    // Validate app ownership and deployment connection
    let (app, _) = validate_app_deployment(&state, &auth, &app_name, &job_id).await?;

    state
        .app_repo
        .delete_deployment_by_job_id(&job_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    state.deployment_events.send(app.id).ok();

    Ok(Json(serde_json::json!({ "success": true })))
}
