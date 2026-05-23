pub use crate::application::volumes::*;

use crate::application::deployment::{AppScaleState, resolve_app_scale_state};
use crate::application::vms::{MeshStatus, VmService};
use crate::error::{ApiError, ApiResult, SseResponse};
use crate::workspace::{WorkspaceEvent, WorkspaceEventKind};
use axum::{
    Json,
    extract::{Path, State},
    response::sse::{Event, Sse},
};
use serde::Serialize;
use tokio_stream::StreamExt;

#[derive(Debug, Serialize, rovo::schemars::JsonSchema)]
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
    pub vcpus: i32,
    pub memory_mib: i64,
    pub scale_state: AppScaleState,
}

use futures::Stream;
use std::convert::Infallible;

#[rovo::rovo]
pub async fn app_logs_stream_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(app_name): Path<String>,
) -> ApiResult<SseResponse<impl Stream<Item = Result<Event, Infallible>>>> {
    // 1. Verify app exists and user has access
    let app = state
        .app_repo
        .get_app_by_name(&app_name)
        .await?
        .ok_or_else(|| ApiError::NotFound("App not found".to_string()))?;

    if app.user_id.to_string() != auth.user_id {
        return Err(ApiError::Forbidden);
    }

    let scale_state = resolve_app_scale_state(&state, &app).await;

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
            let enriched = match serde_json::from_slice::<serde_json::Value>(&msg.payload) {
                Ok(serde_json::Value::Object(mut obj)) => {
                    obj.insert("scale_state".to_string(), serde_json::json!(scale_state));
                    serde_json::Value::Object(obj)
                },
                Ok(other) => other,
                Err(_) => serde_json::json!({
                    "line": String::from_utf8_lossy(&msg.payload).to_string(),
                    "timestamp": chrono::Utc::now().timestamp_millis(),
                    "scale_state": scale_state,
                }),
            };

            yield Ok(Event::default().data(enriched.to_string()));
        }
    };

    Ok(SseResponse(
        Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::new()),
    ))
}

#[rovo::rovo]
pub async fn app_metrics_stream_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(app_name): Path<String>,
) -> ApiResult<SseResponse<impl Stream<Item = Result<Event, Infallible>>>> {
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
    let scale_state = resolve_app_scale_state(&state, &app).await;
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

            let enriched = match data {
                serde_json::Value::Object(mut obj) => {
                    obj.insert("scale_state".to_string(), serde_json::json!(scale_state));
                    serde_json::Value::Object(obj)
                },
                other => other,
            };

            yield Ok(Event::default().data(enriched.to_string()));
        }
    };

    Ok(SseResponse(
        Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::new()),
    ))
}

#[derive(Debug, Serialize, rovo::schemars::JsonSchema)]
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
    pub scale_state: AppScaleState,
}

async fn resolve_app_scale_state_by_id(state: &crate::AppState, app_id: &str) -> AppScaleState {
    let Ok(app_uuid) = uuid::Uuid::parse_str(app_id) else {
        return AppScaleState::Active;
    };

    match state.app_repo.get_app(app_uuid).await {
        Ok(Some(app)) => resolve_app_scale_state(state, &app).await,
        _ => AppScaleState::Active,
    }
}

#[rovo::rovo]
#[tracing::instrument(skip(state, auth))]
pub async fn list_active_deployments(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
) -> ApiResult<Json<Vec<LiveDeploymentInfo>>> {
    // 1. Get all running jobs from scheduler via NATS
    use mikrom_proto::scheduler::{ListAppsRequest, ListAppsResponse};

    let nats_req = ListAppsRequest {
        user_id: auth.user_id.clone(),
        status: None, // We'll filter for RUNNING status
    };

    let scheduler_res: anyhow::Result<ListAppsResponse> = state
        .nats
        .with_timeout(std::time::Duration::from_secs(2))
        .request("mikrom.scheduler.list_apps", nats_req)
        .await;

    let scheduler_apps = match scheduler_res {
        Ok(inner) => inner.apps,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to fetch active apps from scheduler");
            Vec::new()
        },
    };

    // 2. Filter for RUNNING and map to LiveDeploymentInfo
    let mut active_deployments = Vec::new();

    // Optimization: Fetch all deployments for the user once to enrich the scheduler list
    let mut user_deployments = std::collections::HashMap::new();
    if let (Ok(_user_uuid), Ok(deps)) = (
        uuid::Uuid::parse_str(&auth.user_id),
        state
            .app_repo
            .list_deployments_by_user(Some(
                uuid::Uuid::parse_str(&auth.user_id).unwrap_or_default(),
            ))
            .await,
    ) {
        for dep in deps {
            user_deployments.insert(dep.id.to_string(), dep);
        }
    }

    for sch_app in scheduler_apps {
        // Only include RUNNING jobs
        if crate::infrastructure::scheduler::status_name(sch_app.status) != "RUNNING" {
            continue;
        }

        // Enrich using the pre-fetched deployments
        let dep = user_deployments.get(&sch_app.deployment_id);
        let vcpus = dep.map(|d| d.vcpus).unwrap_or(1);
        let memory_mib = dep.map(|d| d.memory_mib).unwrap_or(128);
        let scale_state = resolve_app_scale_state_by_id(&state, &sch_app.app_id).await;

        active_deployments.push(LiveDeploymentInfo {
            job_id: sch_app.job_id,
            deployment_id: sch_app.deployment_id,
            app_id: sch_app.app_id,
            app_name: sch_app.app_name,
            image: sch_app.image,
            status: "RUNNING".to_string(),
            host_id: sch_app.host_id,
            vm_id: sch_app.vm_id,
            cpu_usage: sch_app.cpu_usage,
            ram_used_bytes: sch_app.ram_used_bytes,
            tx_bytes: sch_app.tx_bytes,
            rx_bytes: sch_app.rx_bytes,
            ipv6_address: if sch_app.ipv6_address.is_empty() {
                None
            } else {
                Some(sch_app.ipv6_address)
            },
            vcpus,
            memory_mib,
            scale_state,
        });
    }

    Ok(Json(active_deployments))
}

#[rovo::rovo]
#[tracing::instrument(skip(state, auth))]
pub async fn watch_deployments(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
) -> ApiResult<SseResponse<impl Stream<Item = Result<Event, Infallible>>>> {
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
        let auth_user_uuid = uuid::Uuid::parse_str(&auth_user_id).ok();

        // 0. Initial yield: send current state from scheduler (source of truth)
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

        // Optimization: Fetch all deployments for the user once to enrich the scheduler list
        let mut user_deployments = std::collections::HashMap::new();
        if let Ok(deps) = state_clone.app_repo.list_deployments_by_user(auth_user_uuid).await {
            for dep in deps {
                user_deployments.insert(dep.id.to_string(), dep);
            }
        }

        for job in scheduler_apps {
            if crate::infrastructure::scheduler::status_name(job.status) != "RUNNING" {
                continue;
            }

            // Enrich using the pre-fetched deployments
            let dep = user_deployments.get(&job.deployment_id);
            let git_hash = dep.and_then(|d| d.git_commit_hash.clone());
            let git_msg = dep.and_then(|d| d.git_commit_message.clone());
            let git_branch = dep.and_then(|d| d.git_branch.clone());
            let vcpus = dep.map(|d| d.vcpus).unwrap_or(1);
            let memory_mib = dep.map(|d| d.memory_mib).unwrap_or(128);

            let scale_state = resolve_app_scale_state_by_id(&state_clone, &job.app_id).await;
            let data = serde_json::json!({
                "job_id": job.job_id,
                "deployment_id": job.deployment_id,
                "app_id": job.app_id,
                "app_name": job.app_name,
                "image": job.image,
                "status": "RUNNING",
                "git_commit_hash": git_hash,
                "git_commit_message": git_msg,
                "git_branch": git_branch,
                "host_id": job.host_id,
                "vm_id": job.vm_id,
                "ipv6_address": job.ipv6_address,
                "vcpus": vcpus,
                "memory_mib": memory_mib,
                "cpu_usage": job.cpu_usage,
                "ram_used_bytes": job.ram_used_bytes,
                "tx_bytes": job.tx_bytes,
                "rx_bytes": job.rx_bytes,
                "scale_state": scale_state,
                "scheduled_at": 0,
                "started_at": 0,
                "stopped_at": 0,
                "error_message": "",
            });
            if let Ok(json) = serde_json::to_string(&data) {
                yield Ok::<Event, std::convert::Infallible>(Event::default().data(json));
            }
        }

        loop {
            tokio::select! {
                // 1. Cluster-wide events from NATS
                Some(msg) = nats_stream.next() => {
                    use prost::Message;
                    use mikrom_proto::scheduler::AppInfo;
                    if let Some(job) = AppInfo::decode(&msg.payload[..]).ok().filter(|j| j.user_id == auth_user_id) {
                            let mut vcpus = 1;
                            let mut memory_mib = 128;

                            if let Ok(Some(dep)) = match uuid::Uuid::parse_str(&job.deployment_id) {
                                Ok(id) => state_clone.app_repo.get_deployment(id).await,
                                Err(_) => Ok(None),
                            } {
                                vcpus = dep.vcpus;
                                memory_mib = dep.memory_mib;
                            }

                            let data = serde_json::json!({
                                "job_id": job.job_id,
                                "deployment_id": job.deployment_id,
                                "app_id": job.app_id,
                                "app_name": job.app_name,
                                "image": job.image,
                                "status": crate::infrastructure::scheduler::status_name(job.status),
                                "host_id": job.host_id,
                                "vm_id": job.vm_id,
                                "ipv6_address": job.ipv6_address,
                                "vcpus": vcpus,
                                "memory_mib": memory_mib,
                                "cpu_usage": job.cpu_usage,
                                "ram_used_bytes": job.ram_used_bytes,
                                "tx_bytes": job.tx_bytes,
                                "rx_bytes": job.rx_bytes,
                                "scale_state": resolve_app_scale_state_by_id(&state_clone, &job.app_id).await,
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
                    if let Some(Ok(app_id)) = res {
                        let app_res = state_clone.app_repo.get_app(app_id).await;
                        let deps_res = state_clone.app_repo.list_deployments_by_app(app_id).await;

                        if let (Ok(Some(app)), Ok(deps)) = (app_res, deps_res) {
                            for dep in deps {
                                if ["RUNNING", "DRAINING", "BUILDING", "SCHEDULED", "PAUSED", "STOPPED", "FAILED"].contains(&dep.status.as_str()) {
                                    let data = serde_json::json!({
                                        "job_id": dep.job_id.clone().unwrap_or_default(),
                                        "deployment_id": dep.id.to_string(),
                                        "app_id": dep.app_id.to_string(),
                                        "app_name": app.name.clone(),
                                        "image": dep.image_tag.clone().unwrap_or_default(),
                                        "status": dep.status,
                                        "git_commit_hash": dep.git_commit_hash,
                                        "git_commit_message": dep.git_commit_message,
                                        "git_branch": dep.git_branch,
                                        "host_id": String::new(),
                                        "vm_id": String::new(),
                                        "ipv6_address": dep.ipv6_address,
                                        "vcpus": dep.vcpus,
                                        "memory_mib": dep.memory_mib,
                                        "cpu_usage": 0.0,
                                        "ram_used_bytes": 0,
                                        "tx_bytes": 0,
                                        "rx_bytes": 0,
                                        "scale_state": resolve_app_scale_state(&state_clone, &app).await,
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
                },
                // 3. Periodic refresh (Brute force fallback)
                _ = interval.tick() => {
                    use mikrom_proto::scheduler::{ListAppsRequest, ListAppsResponse};

                    let nats_req = ListAppsRequest {
                        user_id: auth_user_id.clone(),
                        status: None,
                    };

                    let scheduler_res = state_clone
                        .nats
                        .with_timeout(std::time::Duration::from_secs(2))
                        .request::<ListAppsRequest, ListAppsResponse>("mikrom.scheduler.list_apps", nats_req)
                        .await;

                    if let Ok(inner) = scheduler_res {
                        // Batch fetch deployments for enrichment
                        let mut user_deployments = std::collections::HashMap::new();
                        if let Ok(deps) = state_clone.app_repo.list_deployments_by_user(auth_user_uuid).await {
                            for dep in deps {
                                user_deployments.insert(dep.id.to_string(), dep);
                            }
                        }

                        for job in inner.apps {
                             let status = crate::infrastructure::scheduler::status_name(job.status);

                             let dep = user_deployments.get(&job.deployment_id);
                             let vcpus = dep.map(|d| d.vcpus).unwrap_or(1);
                             let memory_mib = dep.map(|d| d.memory_mib).unwrap_or(128);

                             let data = serde_json::json!({
                                "job_id": job.job_id,
                                "deployment_id": job.deployment_id,
                                "app_id": job.app_id,
                                "app_name": job.app_name,
                                "image": job.image,
                                "status": status,
                                "host_id": job.host_id,
                                "vm_id": job.vm_id,
                                "ipv6_address": job.ipv6_address,
                                "vcpus": vcpus,
                                "memory_mib": memory_mib,
                                "cpu_usage": job.cpu_usage,
                                "ram_used_bytes": job.ram_used_bytes,
                                "tx_bytes": job.tx_bytes,
                                "rx_bytes": job.rx_bytes,
                                "scale_state": resolve_app_scale_state_by_id(&state_clone, &job.app_id).await,
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
                        // Fallback to DB ONLY if scheduler is unreachable
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
                                                "vcpus": dep.vcpus,
                                                "memory_mib": dep.memory_mib,
                                                "cpu_usage": 0.0,
                                                "ram_used_bytes": 0,
                                                "scale_state": resolve_app_scale_state(&state_clone, &app).await,
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

    Ok(SseResponse(
        Sse::new(stream).keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(std::time::Duration::from_secs(5))
                .text("keep-alive"),
        ),
    ))
}

#[rovo::rovo]
#[tracing::instrument(skip(state), fields(app_name = %app_name, job_id = %job_id))]
pub async fn get_deployment_status(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, job_id)): Path<(String, String)>,
) -> ApiResult<Json<LiveDeploymentStatus>> {
    use mikrom_proto::scheduler::{AppStatusRequest, AppStatusResponse};

    let (app, dep) =
        VmService::validate_app_deployment(&state, &auth.user_id, &app_name, &job_id).await?;

    // If it's a temporary ID from BUILDING/SCHEDULED phase
    if job_id.starts_with("temp-") {
        let scale_state = resolve_app_scale_state(&state, &app).await;
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
            scale_state,
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

    let scale_state = resolve_app_scale_state(&state, &app).await;

    let deployment_status = LiveDeploymentStatus {
        job_id: inner.job_id,
        deployment_id: dep.id.to_string(),
        app_id: dep.app_id.to_string(),
        app_name: app.name,
        image: dep.image_tag.unwrap_or_default(),
        status: crate::infrastructure::scheduler::status_name(inner.status).to_string(),
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
        scale_state,
    };

    Ok(Json(deployment_status))
}

#[rovo::rovo]
pub async fn get_deployment_logs(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, job_id)): Path<(String, String)>,
) -> ApiResult<SseResponse<impl Stream<Item = Result<Event, Infallible>>>> {
    // 1. Validate app ownership and deployment connection
    let _ = VmService::validate_app_deployment(&state, &auth.user_id, &app_name, &job_id).await?;

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

    Ok(SseResponse(
        Sse::new(stream).keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(std::time::Duration::from_secs(1))
                .text("keep-alive"),
        ),
    ))
}

use crate::application::deployment::DeploymentService;

#[rovo::rovo]
#[tracing::instrument(skip(state), fields(app_name = %app_name, job_id = %job_id))]
pub async fn pause_deployment(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, job_id)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    // Validate app ownership and deployment connection
    let (app, deployment) =
        VmService::validate_app_deployment(&state, &auth.user_id, &app_name, &job_id).await?;

    let success =
        DeploymentService::pause_deployment(&state, &app, &deployment, auth.user_id).await?;

    if success {
        Ok(Json(
            serde_json::json!({ "success": true, "message": "Paused" }),
        ))
    } else {
        Err(ApiError::BadRequest("Failed to pause".to_string()))
    }
}

#[rovo::rovo]
#[tracing::instrument(skip(state), fields(app_name = %app_name, job_id = %job_id))]
pub async fn resume_deployment(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, job_id)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    // Validate app ownership and deployment connection
    let (app, deployment) =
        VmService::validate_app_deployment(&state, &auth.user_id, &app_name, &job_id).await?;

    let success =
        DeploymentService::resume_deployment(&state, &app, &deployment, auth.user_id).await?;

    if success {
        Ok(Json(
            serde_json::json!({ "success": true, "message": "Resumed" }),
        ))
    } else {
        Err(ApiError::BadRequest("Failed to resume".to_string()))
    }
}

#[rovo::rovo]
#[tracing::instrument(skip(state), fields(app_name = %app_name, job_id = %job_id))]
pub async fn stop_deployment(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, job_id)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    // Validate app ownership and deployment connection
    let (app, deployment) =
        VmService::validate_app_deployment(&state, &auth.user_id, &app_name, &job_id).await?;

    let (success, message) =
        DeploymentService::stop_deployment(&state, &app, &deployment, auth.user_id).await?;

    if success {
        Ok(Json(
            serde_json::json!({ "success": true, "message": message }),
        ))
    } else {
        Err(ApiError::NotFound(message))
    }
}

#[rovo::rovo]
#[tracing::instrument(skip(state), fields(app_name = %app_name, job_id = %job_id))]
pub async fn delete_deployment_record(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, job_id)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    // Validate app ownership and deployment connection
    let (app, _) =
        VmService::validate_app_deployment(&state, &auth.user_id, &app_name, &job_id).await?;

    DeploymentService::delete_deployment_record(&state, &app, job_id).await?;

    Ok(Json(serde_json::json!({ "success": true })))
}

#[rovo::rovo]
pub async fn get_mesh_status_handler(
    _auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
) -> ApiResult<Json<MeshStatus>> {
    let mesh_status = state.mesh_status.subscribe();
    Ok(Json(mesh_status.borrow().clone()))
}

#[rovo::rovo]
pub async fn mesh_status_stream_handler(
    _auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
) -> ApiResult<SseResponse<impl Stream<Item = Result<Event, Infallible>>>> {
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

    Ok(SseResponse(
        Sse::new(stream).keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(std::time::Duration::from_secs(10))
                .text("keep-alive"),
        ),
    ))
}

#[derive(Debug, serde::Deserialize, rovo::schemars::JsonSchema)]
pub struct CreateSecurityRuleRequest {
    pub protocol: String,
    pub port_start: u32,
    pub port_end: u32,
    pub action: String,
}

#[rovo::rovo]
pub async fn list_security_rules_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(app_name): Path<String>,
) -> ApiResult<Json<Vec<crate::domain::SecurityRule>>> {
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

#[rovo::rovo]
pub async fn create_security_rule_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(app_name): Path<String>,
    Json(payload): Json<CreateSecurityRuleRequest>,
) -> ApiResult<(axum::http::StatusCode, Json<crate::domain::SecurityRule>)> {
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
        volume_id: None,
        resource_id: None,
    });

    Ok((axum::http::StatusCode::CREATED, Json(rule)))
}

#[rovo::rovo]
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
        volume_id: None,
        resource_id: Some(rule_id),
    });

    Ok(Json(serde_json::json!({ "success": true })))
}

// ── VM Runtime Operations ──────────────────────────────────────────────────

#[derive(Debug, serde::Deserialize, rovo::schemars::JsonSchema)]
pub struct SnapshotNameRequest {
    pub snapshot_name: String,
}

#[derive(Debug, serde::Deserialize, rovo::schemars::JsonSchema)]
pub struct AttachVolumeRuntimeRequest {
    pub volume_id: String,
    pub mount_point: String,
    #[serde(default)]
    pub read_only: bool,
}

#[derive(Debug, serde::Deserialize, rovo::schemars::JsonSchema)]
pub struct MigrationStartRequest {
    pub target_host: String,
    pub target_uri: String,
}

#[derive(Debug, serde::Deserialize, rovo::schemars::JsonSchema)]
pub struct BalloonSetRequest {
    pub target_memory_mib: u32,
}

#[rovo::rovo]
pub async fn vm_snapshot_create_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, job_id)): Path<(String, String)>,
    Json(payload): Json<SnapshotNameRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = VmService::validate_app_deployment(&state, &auth.user_id, &app_name, &job_id).await?;
    let (success, message) =
        VmService::create_snapshot(&state, auth.user_id, job_id, payload.snapshot_name).await?;
    Ok(Json(serde_json::json!({
        "success": success,
        "message": message,
    })))
}

#[rovo::rovo]
pub async fn vm_snapshot_restore_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, job_id, snapshot_name)): Path<(String, String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = VmService::validate_app_deployment(&state, &auth.user_id, &app_name, &job_id).await?;
    let (success, message) =
        VmService::restore_snapshot(&state, auth.user_id, job_id, snapshot_name).await?;
    Ok(Json(serde_json::json!({
        "success": success,
        "message": message,
    })))
}

#[rovo::rovo]
pub async fn vm_snapshot_delete_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, job_id, snapshot_name)): Path<(String, String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = VmService::validate_app_deployment(&state, &auth.user_id, &app_name, &job_id).await?;
    let (success, message) =
        VmService::delete_snapshot(&state, auth.user_id, job_id, snapshot_name).await?;
    Ok(Json(serde_json::json!({
        "success": success,
        "message": message,
    })))
}

#[rovo::rovo]
pub async fn vm_snapshot_list_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, job_id)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = VmService::validate_app_deployment(&state, &auth.user_id, &app_name, &job_id).await?;
    let (success, message, snapshots) =
        VmService::list_snapshots(&state, auth.user_id, job_id).await?;

    let snapshots_json: Vec<serde_json::Value> = snapshots
        .into_iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id,
                "name": s.name,
                "created_at": s.created_at,
                "size_bytes": s.size_bytes,
                "vm_status": s.vm_status,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "success": success,
        "message": message,
        "snapshots": snapshots_json,
    })))
}

#[rovo::rovo]
pub async fn attach_volume_runtime_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, job_id)): Path<(String, String)>,
    Json(payload): Json<AttachVolumeRuntimeRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = VmService::validate_app_deployment(&state, &auth.user_id, &app_name, &job_id).await?;
    let (success, message) = VmService::attach_volume(
        &state,
        auth.user_id,
        job_id,
        payload.volume_id,
        payload.mount_point,
        payload.read_only,
    )
    .await?;
    Ok(Json(serde_json::json!({
        "success": success,
        "message": message,
    })))
}

#[rovo::rovo]
pub async fn detach_volume_runtime_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, job_id)): Path<(String, String)>,
    Json(payload): Json<serde_json::Value>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = VmService::validate_app_deployment(&state, &auth.user_id, &app_name, &job_id).await?;
    let volume_id = payload
        .get("volume_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::BadRequest("volume_id is required".to_string()))?;

    let (success, message) =
        VmService::detach_volume(&state, auth.user_id, job_id, volume_id.to_string()).await?;

    Ok(Json(serde_json::json!({
        "success": success,
        "message": message,
    })))
}

#[rovo::rovo]
pub async fn start_migration_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, job_id)): Path<(String, String)>,
    Json(payload): Json<MigrationStartRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = VmService::validate_app_deployment(&state, &auth.user_id, &app_name, &job_id).await?;
    let (success, message) = VmService::start_migration(
        &state,
        auth.user_id,
        job_id,
        payload.target_host,
        payload.target_uri,
    )
    .await?;
    Ok(Json(serde_json::json!({
        "success": success,
        "message": message,
    })))
}

#[rovo::rovo]
pub async fn cancel_migration_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, job_id)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = VmService::validate_app_deployment(&state, &auth.user_id, &app_name, &job_id).await?;
    let (success, message) = VmService::cancel_migration(&state, auth.user_id, job_id).await?;
    Ok(Json(serde_json::json!({
        "success": success,
        "message": message,
    })))
}

#[rovo::rovo]
pub async fn query_migration_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, job_id)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = VmService::validate_app_deployment(&state, &auth.user_id, &app_name, &job_id).await?;
    let (success, message, status) =
        VmService::query_migration(&state, auth.user_id, job_id).await?;
    Ok(Json(serde_json::json!({
        "success": success,
        "message": message,
        "status": status,
    })))
}

#[rovo::rovo]
pub async fn set_balloon_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, job_id)): Path<(String, String)>,
    Json(payload): Json<BalloonSetRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = VmService::validate_app_deployment(&state, &auth.user_id, &app_name, &job_id).await?;
    let (success, message) =
        VmService::set_balloon(&state, auth.user_id, job_id, payload.target_memory_mib).await?;
    Ok(Json(serde_json::json!({
        "success": success,
        "message": message,
    })))
}

#[rovo::rovo]
pub async fn query_balloon_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_name, job_id)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = VmService::validate_app_deployment(&state, &auth.user_id, &app_name, &job_id).await?;
    let (success, message, actual, max) =
        VmService::query_balloon(&state, auth.user_id, job_id).await?;
    Ok(Json(serde_json::json!({
        "success": success,
        "message": message,
        "actual_memory_mib": actual,
        "max_memory_mib": max,
    })))
}
