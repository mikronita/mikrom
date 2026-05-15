pub mod volumes;
pub use volumes::*;

use crate::error::{ApiError, ApiResult};
use crate::models::app::SecurityRule;
use crate::repositories::app_repository::UpdateDeploymentParams;
use crate::workspace::{WorkspaceEvent, WorkspaceEventKind};
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
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub ipv6_address: Option<String>,
}

use futures::Stream;
use std::convert::Infallible;

#[utoipa::path(
    get,
    path = "/v1/apps/{app_name}/logs/stream",
    params(
        ("app_name" = String, Path, description = "Application name")
    ),
    responses(
        (status = 200, description = "SSE stream of application logs", content_type = "text/event-stream"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Application not found", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
pub async fn app_logs_stream_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(app_name): Path<String>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    // 1. Verify app exists and user has access
    let app = state
        .app_repo
        .get_app_by_name(&app_name)
        .await?
        .ok_or_else(|| ApiError::NotFound("App not found".to_string()))?;

    if app.user_id.to_string() != auth.user_id {
        return Err(ApiError::Forbidden);
    }

    // 2. Subscribe to NATS for all logs of this app
    // Subject pattern: mikrom.logs.<app_id>.>
    let nats_sub = state
        .nats
        .subscribe(format!("mikrom.logs.{}.>", app.id))
        .await
        .map_err(|e| ApiError::Internal(format!("NATS subscription failed: {e}")))?;

    let stream = async_stream::stream! {
        let mut nats_stream = nats_sub;
        while let Some(msg) = nats_stream.next().await {
             if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&msg.payload) {
                 yield Ok(Event::default().data(json.to_string()));
             }
        }
    };

    Ok(Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::new()))
}

#[utoipa::path(
    get,
    path = "/v1/apps/{app_name}/metrics/stream",
    params(
        ("app_name" = String, Path, description = "Application name")
    ),
    responses(
        (status = 200, description = "SSE stream of application metrics", content_type = "text/event-stream"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Application not found", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
pub async fn app_metrics_stream_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(app_name): Path<String>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    // 1. Verify app exists and user has access
    let app = state
        .app_repo
        .get_app_by_name(&app_name)
        .await?
        .ok_or_else(|| ApiError::NotFound("App not found".to_string()))?;

    if app.user_id.to_string() != auth.user_id {
        return Err(ApiError::Forbidden);
    }

    let app_id = app.id.to_string();
    let active_deployment_id = app.active_deployment_id.map(|id| id.to_string());
    let mut nats_sub = state
        .nats
        .subscribe(format!("mikrom.metrics.{}.>", app_id))
        .await
        .map_err(|e| ApiError::Internal(format!("NATS subscription failed: {e}")))?;

    let stream = async_stream::stream! {
        while let Some(msg) = nats_sub.next().await {
            let Ok(data) = serde_json::from_slice::<serde_json::Value>(&msg.payload) else {
                continue;
            };

            if let Some(active_deployment_id) = &active_deployment_id
                && data
                    .get("deployment_id")
                    .and_then(|value| value.as_str())
                    != Some(active_deployment_id.as_str())
            {
                continue;
            }

            if data.get("status").and_then(|value| value.as_str()) != Some("RUNNING") {
                continue;
            }

            yield Ok(Event::default().data(data.to_string()));
        }
    };

    Ok(Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::new()))
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
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub ipv6_address: Option<String>,
    pub vcpus: i32,
    pub memory_mib: i64,
}

#[utoipa::path(
    get,
    path = "/v1/deployments/active",
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
    let user_id =
        uuid::Uuid::parse_str(&auth.user_id).map_err(|e| ApiError::Internal(e.to_string()))?;

    // 1. Get all deployments for this user from DB
    let deployments = state
        .app_repo
        .list_deployments_by_user(Some(user_id))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    // 2. Try to get real-time status from scheduler for active ones
    let mut scheduler_apps = HashMap::new();

    use mikrom_proto::scheduler::{ListAppsRequest, ListAppsResponse};

    let nats_req = ListAppsRequest {
        user_id: auth.user_id.clone(),
        status: None,
    };

    let scheduler_res: anyhow::Result<ListAppsResponse> = state
        .nats
        .with_timeout(std::time::Duration::from_secs(2))
        .request("mikrom.scheduler.list_apps", nats_req)
        .await;

    if let Ok(inner) = scheduler_res {
        for app in inner.apps {
            scheduler_apps.insert(app.job_id.clone(), app);
        }
    }

    // 3. Map deployments to LiveDeploymentInfo, using scheduler data if available
    let mut active_deployments = Vec::new();
    for dep in deployments {
        let (status, host_id, vm_id, cpu_usage, ram_used_bytes, tx_bytes, rx_bytes, ipv6_address) =
            if let Some(job_id_real) = &dep.job_id {
                if let Some(sch_app) = scheduler_apps.get(job_id_real) {
                    (
                        crate::scheduler::status_name(sch_app.status).to_string(),
                        sch_app.host_id.clone(),
                        sch_app.vm_id.clone(),
                        sch_app.cpu_usage,
                        sch_app.ram_used_bytes,
                        sch_app.tx_bytes,
                        sch_app.rx_bytes,
                        if sch_app.ipv6_address.is_empty() {
                            dep.ipv6_address.clone()
                        } else {
                            Some(sch_app.ipv6_address.clone())
                        },
                    )
                } else {
                    (
                        dep.status.clone(),
                        String::new(),
                        String::new(),
                        0.0,
                        0,
                        0,
                        0,
                        dep.ipv6_address.clone(),
                    )
                }
            } else {
                (
                    dep.status.clone(),
                    String::new(),
                    String::new(),
                    0.0,
                    0,
                    0,
                    0,
                    dep.ipv6_address.clone(),
                )
            };

        // Only include deployments that are actually RUNNING in the cluster or marked as RUNNING in DB
        if status != "RUNNING" {
            continue;
        }

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
            tx_bytes,
            rx_bytes,
            ipv6_address,
        });
    }

    Ok(Json(active_deployments))
}

#[utoipa::path(
    get,
    path = "/v1/deployments/events",
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
        .nats
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
        let auth_user_uuid = uuid::Uuid::parse_str(&auth_user_id).ok();
        if let Ok(apps) = state_clone.app_repo.list_apps_by_user(auth_user_uuid).await {
            for app in apps {
                if let Ok(deps) = state_clone.app_repo.list_deployments_by_app(app.id).await {
                    for dep in deps {
                        if ["RUNNING", "DRAINING", "BUILDING", "SCHEDULED", "PAUSED", "STOPPED", "FAILED"].contains(&dep.status.as_str()) {
                            let data = serde_json::json!({
                                "job_id": dep.job_id.clone().unwrap_or_default(),
                                "deployment_id": dep.id.to_string(),
                                "app_id": dep.app_id.to_string(),
                                "app_name": app.name.clone(),
                                "image": dep.image_tag.clone().unwrap_or_default(),
                                "status": dep.status,
                                "host_id": String::new(),
                                "vm_id": String::new(),
                                "ipv6_address": dep.ipv6_address,
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
                                "ipv6_address": job.ipv6_address,
                                "cpu_usage": job.cpu_usage,
                                "ram_used_bytes": job.ram_used_bytes,
                                "tx_bytes": job.tx_bytes,
                                "rx_bytes": job.rx_bytes,
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
                            if ["RUNNING", "DRAINING", "BUILDING", "SCHEDULED", "PAUSED", "STOPPED", "FAILED"].contains(&dep.status.as_str()) {
                                let data = serde_json::json!({
                                    "job_id": dep.job_id.clone().unwrap_or_default(),
                                    "deployment_id": dep.id.to_string(),
                                    "app_id": dep.app_id.to_string(),
                                    "app_name": "",
                                    "image": dep.image_tag.clone().unwrap_or_default(),
                                    "status": dep.status,
                                    "host_id": String::new(),
                                    "vm_id": String::new(),
                                    "ipv6_address": dep.ipv6_address,
                                    "cpu_usage": 0.0,
                                    "ram_used_bytes": 0,
                                    "tx_bytes": 0,
                                    "rx_bytes": 0,
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

                    let nats_req = ListAppsRequest {
                        user_id: auth_user_id.clone(),
                        status: None,
                    };

                    let scheduler_apps = state_clone
                        .nats
                        .with_timeout(std::time::Duration::from_secs(2))
                        .request::<ListAppsRequest, ListAppsResponse>("mikrom.scheduler.list_apps", nats_req)
                        .await
                        .ok()
                        .map(|r| r.apps)
                        .unwrap_or_default();

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
                                "ipv6_address": job.ipv6_address,
                                "cpu_usage": job.cpu_usage,
                                "ram_used_bytes": job.ram_used_bytes,
                                "tx_bytes": job.tx_bytes,
                                "rx_bytes": job.rx_bytes,
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
                        if let Ok(apps) = state_clone.app_repo.list_apps_by_user(auth_user_uuid).await {
                            for app in apps {
                                if let Ok(deps) = state_clone.app_repo.list_deployments_by_app(app.id).await {
                                    for dep in deps {
                                        if ["RUNNING", "DRAINING", "BUILDING", "SCHEDULED", "PAUSED", "STOPPED", "FAILED"].contains(&dep.status.as_str()) {
                                            let data = serde_json::json!({
                                                "job_id": dep.job_id.clone().unwrap_or_default(),
                                                "deployment_id": dep.id.to_string(),
                                                "app_id": dep.app_id.to_string(),
                                                "app_name": app.name.clone(),
                                                "image": dep.image_tag.clone().unwrap_or_default(),
                                                "status": dep.status,
                                                "host_id": String::new(),
                                                "vm_id": String::new(),
                                                "ipv6_address": dep.ipv6_address,
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
    path = "/v1/apps/{app_name}/deployments/{job_id}",
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
            tx_bytes: 0,
            rx_bytes: 0,
            vcpus: dep.vcpus,
            memory_mib: dep.memory_mib,
            ipv6_address: dep.ipv6_address,
        }));
    }

    let nats_req = AppStatusRequest {
        job_id: job_id.clone(),
        user_id: auth.user_id.clone(),
    };

    let inner: AppStatusResponse = state
        .nats
        .request("mikrom.scheduler.get_job", nats_req)
        .await
        .map_err(|e| ApiError::Internal(format!("NATS request failed: {}", e)))?;

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
        tx_bytes: inner.tx_bytes,
        rx_bytes: inner.rx_bytes,
        ipv6_address: if !inner.ipv6_address.is_empty() {
            Some(inner.ipv6_address)
        } else {
            dep.ipv6_address
        },
        vcpus: dep.vcpus,
        memory_mib: dep.memory_mib,
    };

    Ok(Json(deployment_status))
}

#[utoipa::path(
    get,
    path = "/v1/apps/{app_name}/deployments/{job_id}/logs",
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
    use mikrom_proto::scheduler::{AppStatusRequest, AppStatusResponse};

    let nats_req = AppStatusRequest {
        job_id: job_id.clone(),
        user_id: auth.user_id.clone(),
    };

    let inner: AppStatusResponse = state
        .nats
        .request("mikrom.scheduler.get_job", nats_req)
        .await
        .map_err(|e| ApiError::Internal(format!("NATS request failed: {}", e)))?;

    let vm_id = inner.vm_id;
    if vm_id.is_empty() {
        return Err(ApiError::BadRequest(
            "VM is not yet active or assigned".to_string(),
        ));
    }

    let subject = format!("mikrom.logs.{}", vm_id);
    let subscription = state
        .nats
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
    path = "/v1/apps/{app_name}/deployments/{job_id}/pause",
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
    let job_id_for_event = job_id.clone();

    tracing::info!(
        app = %app.name,
        job_id = %job_id,
        user_id = %auth.user_id,
        origin = "manual_pause",
        "Forwarding pause request to scheduler"
    );

    let success = state
        .scheduler
        .pause_app(job_id.clone(), auth.user_id.clone())
        .await
        .map_err(ApiError::Scheduler)?;

    if success {
        tracing::info!(
            app = %app.name,
            job_id = %job_id,
            user_id = %auth.user_id,
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
                    job_id: Some(job_id),
                    image_tag: deployment.image_tag,
                    build_id: deployment.build_id,
                    git_commit_hash: deployment.git_commit_hash,
                    git_commit_message: deployment.git_commit_message,
                    git_branch: deployment.git_branch,
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
            resource_id: Some(job_id_for_event),
        });

        Ok(Json(
            serde_json::json!({ "success": true, "message": "Paused" }),
        ))
    } else {
        Err(ApiError::BadRequest("Failed to pause".to_string()))
    }
}

#[utoipa::path(
    post,
    path = "/v1/apps/{app_name}/deployments/{job_id}/resume",
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
    let job_id_for_event = job_id.clone();

    let success = state
        .scheduler
        .resume_app(job_id.clone(), auth.user_id.clone())
        .await
        .map_err(ApiError::Scheduler)?;

    if success {
        // Update database status
        let _ = state
            .app_repo
            .update_deployment(
                deployment.id,
                UpdateDeploymentParams {
                    status: Some("RUNNING".to_string()),
                    job_id: Some(job_id),
                    image_tag: deployment.image_tag,
                    build_id: deployment.build_id,
                    git_commit_hash: deployment.git_commit_hash,
                    git_commit_message: deployment.git_commit_message,
                    git_branch: deployment.git_branch,
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
            resource_id: Some(job_id_for_event),
        });

        Ok(Json(
            serde_json::json!({ "success": true, "message": "Resumed" }),
        ))
    } else {
        Err(ApiError::BadRequest("Failed to resume".to_string()))
    }
}

#[utoipa::path(
    delete,
    path = "/v1/apps/{app_name}/deployments/{job_id}",
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
    let job_id_for_event = job_id.clone();

    use mikrom_proto::scheduler::{CancelRequest, CancelResponse};

    let nats_req = CancelRequest {
        job_id: job_id.clone(),
        user_id: auth.user_id.clone(),
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
                    job_id: Some(job_id),
                    image_tag: deployment.image_tag,
                    build_id: deployment.build_id,
                    git_commit_hash: deployment.git_commit_hash,
                    git_commit_message: deployment.git_commit_message,
                    git_branch: deployment.git_branch,
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
            resource_id: Some(job_id_for_event),
        });
        Ok(Json(
            serde_json::json!({ "success": true, "message": inner.message }),
        ))
    } else {
        Err(ApiError::NotFound(inner.message))
    }
}

#[utoipa::path(
    delete,
    path = "/v1/apps/{app_name}/deployments/{job_id}/delete",
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
    state.publish_workspace_event(WorkspaceEvent {
        kind: WorkspaceEventKind::DeploymentChanged,
        user_id: Some(app.user_id),
        app_id: Some(app.id),
        app_name: Some(app.name),
        deployment_id: None,
        resource_id: Some(job_id.clone()),
    });

    Ok(Json(serde_json::json!({ "success": true })))
}

#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct MeshStatus {
    pub workers: Vec<crate::models::worker::Worker>,
    pub total_workers: usize,
}

#[utoipa::path(
    get,
    path = "/v1/networking/mesh",
    responses(
        (status = 200, description = "Mesh network status", body = MeshStatus),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "networking",
    security(
        ("jwt" = [])
    )
)]
pub async fn get_mesh_status_handler(
    _auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
) -> ApiResult<Json<MeshStatus>> {
    let mesh_status = state.mesh_status.subscribe();
    Ok(Json(mesh_status.borrow().clone()))
}

async fn fetch_mesh_status(state: &crate::AppState) -> ApiResult<MeshStatus> {
    use crate::models::worker::Worker;

    let workers = state
        .scheduler
        .list_workers()
        .await
        .map_err(ApiError::Internal)?;

    Ok(MeshStatus {
        total_workers: workers.workers.len(),
        workers: workers.workers.into_iter().map(Worker::from).collect(),
    })
}

pub async fn prime_mesh_status_cache(state: &crate::AppState) -> ApiResult<()> {
    let snapshot = fetch_mesh_status(state).await?;
    let _ = state.mesh_status.send(snapshot);
    Ok(())
}

async fn refresh_mesh_status_cache(state: &crate::AppState) -> ApiResult<MeshStatus> {
    let snapshot = fetch_mesh_status(state).await?;
    let _ = state.mesh_status.send(snapshot.clone());
    Ok(snapshot)
}

pub async fn start_mesh_status_tracker(state: crate::AppState) {
    let mut worker_heartbeat_sub = match state
        .nats
        .subscribe("mikrom.scheduler.worker.heartbeat")
        .await
    {
        Ok(sub) => sub,
        Err(err) => {
            tracing::error!("Failed to subscribe to worker heartbeats: {}", err);
            return;
        },
    };
    let mut router_heartbeat_sub = match state
        .nats
        .subscribe("mikrom.scheduler.router.heartbeat")
        .await
    {
        Ok(sub) => sub,
        Err(err) => {
            tracing::error!("Failed to subscribe to router heartbeats: {}", err);
            return;
        },
    };
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));

    loop {
        tokio::select! {
            Some(_) = worker_heartbeat_sub.next() => {
                if let Err(err) = refresh_mesh_status_cache(&state).await {
                    tracing::warn!("failed to refresh mesh status after worker heartbeat: {}", err);
                }
            },
            Some(_) = router_heartbeat_sub.next() => {
                if let Err(err) = refresh_mesh_status_cache(&state).await {
                    tracing::warn!("failed to refresh mesh status after router heartbeat: {}", err);
                }
            },
            _ = interval.tick() => {
                if let Err(err) = refresh_mesh_status_cache(&state).await {
                    tracing::warn!("failed to refresh mesh status on interval: {}", err);
                }
            },
            else => break,
        }
    }
}

#[utoipa::path(
    get,
    path = "/v1/networking/mesh/stream",
    responses(
        (status = 200, description = "SSE stream of mesh status updates", content_type = "text/event-stream"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "networking",
    security(
        ("jwt" = [])
    )
)]
pub async fn mesh_status_stream_handler(
    _auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let mut rx = state.mesh_status.subscribe();

    let stream = async_stream::stream! {
        let snapshot = rx.borrow().clone();
        if let Ok(data) = serde_json::to_string(&snapshot) {
            yield Ok(Event::default().data(data));
        }

        loop {
            if rx.changed().await.is_err() {
                break;
            }

            let snapshot = rx.borrow_and_update().clone();
            if let Ok(data) = serde_json::to_string(&snapshot) {
                yield Ok(Event::default().data(data));
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(10))
            .text("keep-alive"),
    ))
}

#[derive(Debug, serde::Deserialize, ToSchema)]
pub struct CreateSecurityRuleRequest {
    pub protocol: String,
    pub port_start: i32,
    pub port_end: i32,
    pub action: String,
}

#[utoipa::path(
    get,
    path = "/v1/apps/{app_name}/security-groups",
    params(
        ("app_name" = String, Path, description = "Application name")
    ),
    responses(
        (status = 200, description = "List of security rules", body = [SecurityRule]),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Application not found", body = crate::error::ErrorResponse)
    ),
    tag = "networking",
    security(
        ("jwt" = [])
    )
)]
pub async fn list_security_rules_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(app_name): Path<String>,
) -> ApiResult<Json<Vec<crate::models::app::SecurityRule>>> {
    let app = state
        .app_repo
        .get_app_by_name(&app_name)
        .await?
        .ok_or_else(|| ApiError::NotFound("App not found".to_string()))?;

    if app.user_id.to_string() != auth.user_id {
        return Err(ApiError::Forbidden);
    }

    let rules = state.app_repo.list_security_rules(app.id).await?;
    Ok(Json(rules))
}

#[utoipa::path(
    post,
    path = "/v1/apps/{app_name}/security-groups",
    request_body = CreateSecurityRuleRequest,
    params(
        ("app_name" = String, Path, description = "Application name")
    ),
    responses(
        (status = 201, description = "Security rule created", body = SecurityRule),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Application not found", body = crate::error::ErrorResponse)
    ),
    tag = "networking",
    security(
        ("jwt" = [])
    )
)]
pub async fn create_security_rule_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(app_name): Path<String>,
    Json(payload): Json<CreateSecurityRuleRequest>,
) -> ApiResult<(
    axum::http::StatusCode,
    Json<crate::models::app::SecurityRule>,
)> {
    let app = state
        .app_repo
        .get_app_by_name(&app_name)
        .await?
        .ok_or_else(|| ApiError::NotFound("App not found".to_string()))?;

    if app.user_id.to_string() != auth.user_id {
        return Err(ApiError::Forbidden);
    }

    let rule = state
        .app_repo
        .create_security_rule(
            app.id,
            payload.protocol,
            payload.port_start,
            payload.port_end,
            payload.action,
        )
        .await?;

    // Notify scheduler to apply rules to active VMs
    let nats_req = mikrom_proto::scheduler::UpdateSecurityGroupsRequest {
        app_id: app.id.to_string(),
        user_id: auth.user_id.clone(),
        rules: Vec::new(), // Rules will be fetched by scheduler from DB
    };

    let _: anyhow::Result<mikrom_proto::scheduler::UpdateSecurityGroupsResponse> = state
        .nats
        .request("mikrom.scheduler.update_security_groups", nats_req)
        .await;

    state.publish_workspace_event(WorkspaceEvent {
        kind: WorkspaceEventKind::SecurityRulesChanged,
        user_id: Some(app.user_id),
        app_id: Some(app.id),
        app_name: Some(app.name),
        deployment_id: None,
        resource_id: None,
    });

    Ok((axum::http::StatusCode::CREATED, Json(rule)))
}

#[utoipa::path(
    delete,
    path = "/v1/apps/{app_name}/security-groups/{rule_id}",
    params(
        ("app_name" = String, Path, description = "Application name"),
        ("rule_id" = String, Path, description = "Rule UUID")
    ),
    responses(
        (status = 200, description = "Security rule deleted"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Application/Rule not found", body = crate::error::ErrorResponse)
    ),
    tag = "networking",
    security(
        ("jwt" = [])
    )
)]
pub async fn delete_security_rule_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, rule_id)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let app = state
        .app_repo
        .get_app_by_name(&app_name)
        .await?
        .ok_or_else(|| ApiError::NotFound("App not found".to_string()))?;

    if app.user_id.to_string() != auth.user_id {
        return Err(ApiError::Forbidden);
    }

    let rule_uuid =
        uuid::Uuid::parse_str(&rule_id).map_err(|e| ApiError::Internal(e.to_string()))?;

    state.app_repo.delete_security_rule(rule_uuid).await?;

    // Notify scheduler to apply rules to active VMs
    let nats_req = mikrom_proto::scheduler::UpdateSecurityGroupsRequest {
        app_id: app.id.to_string(),
        user_id: auth.user_id.clone(),
        rules: Vec::new(),
    };

    let _: anyhow::Result<mikrom_proto::scheduler::UpdateSecurityGroupsResponse> = state
        .nats
        .request("mikrom.scheduler.update_security_groups", nats_req)
        .await;

    state.publish_workspace_event(WorkspaceEvent {
        kind: WorkspaceEventKind::SecurityRulesChanged,
        user_id: Some(app.user_id),
        app_id: Some(app.id),
        app_name: Some(app.name),
        deployment_id: None,
        resource_id: Some(rule_id),
    });

    Ok(Json(serde_json::json!({ "success": true })))
}
