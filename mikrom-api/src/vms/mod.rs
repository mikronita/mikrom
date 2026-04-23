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
    let channel_res = crate::scheduler::connect(&state.scheduler_config).await;
    let mut scheduler_apps = HashMap::new();

    if let Ok(channel) = channel_res {
        let mut client = mikrom_proto::scheduler::SchedulerServiceClient::new(channel);
        let req = mikrom_proto::scheduler::ListAppsRequest {
            user_id: auth.user_id.clone(),
            status: None,
        };

        if let Ok(resp) = client.list_apps(req).await {
            for app in resp.into_inner().apps {
                scheduler_apps.insert(app.job_id.clone(), app);
            }
        }
    }

    // 3. Map deployments to LiveDeploymentInfo, using scheduler data if available
    let mut active_deployments = Vec::new();
    for dep in deployments {
        // Only show deployments that have a job_id (meaning they were at least attempted to be scheduled)
        if let Some(job_id) = &dep.job_id {
            let (status, host_id, vm_id) = if let Some(sch_app) = scheduler_apps.get(job_id) {
                (
                    crate::scheduler::status_name(sch_app.status).to_string(),
                    sch_app.host_id.clone(),
                    sch_app.vm_id.clone(),
                )
            } else {
                (dep.status.clone(), String::new(), String::new())
            };

            // Get app name from repo (we might need a join or a cache here for performance)
            let app_name = if let Ok(Some(app)) = state.app_repo.get_app(dep.app_id).await {
                app.name
            } else {
                "Unknown".to_string()
            };

            active_deployments.push(LiveDeploymentInfo {
                job_id: job_id.clone(),
                deployment_id: dep.id.to_string(),
                app_id: dep.app_id.to_string(),
                app_name,
                image: dep.image_tag.unwrap_or_default(),
                status,
                host_id,
                vm_id,
            });
        }
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
    let channel = crate::scheduler::connect(&state.scheduler_config)
        .await
        .map_err(ApiError::Scheduler)?;

    let mut client = mikrom_proto::scheduler::SchedulerServiceClient::new(channel);
    let req = mikrom_proto::scheduler::WatchAppsRequest {
        user_id: auth.user_id,
    };

    let resp = client
        .watch_apps(req)
        .await
        .map_err(|e| ApiError::Internal(e.message().to_string()))?;

    let stream = resp.into_inner().map(move |res| match res {
        Ok(msg) => {
            if let Some(app) = msg.app {
                let data = serde_json::json!(LiveDeploymentInfo {
                    job_id: app.job_id,
                    deployment_id: String::new(), // job_id is sufficient for UI reconciliation
                    app_id: app.app_id,
                    app_name: app.app_name,
                    image: app.image,
                    status: crate::scheduler::status_name(app.status).to_string(),
                    host_id: app.host_id,
                    vm_id: app.vm_id,
                })
                .to_string();
                Ok::<Event, std::convert::Infallible>(Event::default().data(data))
            } else {
                Ok::<Event, std::convert::Infallible>(Event::default().comment("keep-alive"))
            }
        },
        Err(e) => {
            tracing::error!("Error in scheduler watch_apps stream: {}", e);
            Ok::<Event, std::convert::Infallible>(Event::default().comment(format!("error: {}", e)))
        },
    });

    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(5))
            .text("keep-alive"),
    ))
}

#[utoipa::path(
    get,
    path = "/deployments/{job_id}",
    params(
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
#[tracing::instrument(skip(state, auth), fields(job_id = %job_id))]
pub async fn get_deployment_status(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(job_id): Path<String>,
) -> ApiResult<Json<LiveDeploymentStatus>> {
    let channel = crate::scheduler::connect(&state.scheduler_config)
        .await
        .map_err(ApiError::Scheduler)?;

    let mut client = mikrom_proto::scheduler::SchedulerServiceClient::new(channel);
    let req = mikrom_proto::scheduler::AppStatusRequest {
        job_id,
        user_id: auth.user_id,
    };

    let resp = client.get_app_status(req).await.map_err(|e| {
        if e.code() == tonic::Code::NotFound {
            ApiError::NotFound("Job not found".to_string())
        } else {
            ApiError::Internal(e.message().to_string())
        }
    })?;

    let inner = resp.into_inner();

    // Fetch deployment to get app_id, image, vcpus, memory
    let deployment = state
        .app_repo
        .get_deployment_by_job_id(&inner.job_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or(ApiError::NotFound(
            "Deployment record not found".to_string(),
        ))?;

    let app_name = if let Ok(Some(app)) = state.app_repo.get_app(deployment.app_id).await {
        app.name
    } else {
        "Unknown".to_string()
    };

    let deployment_status = LiveDeploymentStatus {
        job_id: inner.job_id,
        deployment_id: deployment.id.to_string(),
        app_id: deployment.app_id.to_string(),
        app_name,
        image: deployment.image_tag.unwrap_or_default(),
        status: crate::scheduler::status_name(inner.status).to_string(),
        host_id: inner.host_id,
        vm_id: inner.vm_id,
        scheduled_at: inner.scheduled_at,
        started_at: inner.started_at,
        stopped_at: inner.stopped_at,
        error_message: inner.error_message,
        cpu_usage: inner.cpu_usage,
        ram_used_bytes: inner.ram_used_bytes,
        vcpus: deployment.vcpus,
        memory_mib: deployment.memory_mib,
    };

    Ok(Json(deployment_status))
}

#[utoipa::path(
    get,
    path = "/deployments/{job_id}/logs",
    params(
        ("job_id" = String, Path, description = "Deployment Job ID")
    ),
    responses(
        (status = 200, description = "SSE stream of deployment logs"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
#[tracing::instrument(skip(state, auth), fields(job_id = %job_id))]
pub async fn get_deployment_logs(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(job_id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let channel = crate::scheduler::connect(&state.scheduler_config)
        .await
        .map_err(ApiError::Scheduler)?;

    let mut client = mikrom_proto::scheduler::SchedulerServiceClient::new(channel);
    let req = mikrom_proto::scheduler::GetLogsRequest {
        job_id,
        user_id: auth.user_id,
        follow: true,
    };

    let resp = client
        .get_app_logs(req)
        .await
        .map_err(|e| ApiError::Internal(e.message().to_string()))?;

    let stream = resp.into_inner().map(|res| match res {
        Ok(log) => {
            let data = serde_json::json!({
                "line": log.line,
                "timestamp": log.timestamp,
            })
            .to_string();
            Ok::<Event, std::convert::Infallible>(Event::default().data(data))
        },
        Err(e) => {
            let data = serde_json::json!({
                "line": format!("Error: {}", e),
                "timestamp": chrono::Utc::now().timestamp(),
            })
            .to_string();
            Ok::<Event, std::convert::Infallible>(Event::default().data(data))
        },
    });

    Ok(Sse::new(stream))
}

#[utoipa::path(
    delete,
    path = "/deployments/{job_id}",
    params(
        ("job_id" = String, Path, description = "Deployment Job ID")
    ),
    responses(
        (status = 200, description = "Deployment stopped"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
#[tracing::instrument(skip(state, auth), fields(job_id = %job_id))]
pub async fn stop_deployment(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(job_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let channel = crate::scheduler::connect(&state.scheduler_config)
        .await
        .map_err(ApiError::Scheduler)?;

    let mut client = mikrom_proto::scheduler::SchedulerServiceClient::new(channel);
    let req = mikrom_proto::scheduler::CancelRequest {
        job_id: job_id.clone(),
        user_id: auth.user_id,
    };

    let resp = client
        .cancel_app(req)
        .await
        .map_err(|e| ApiError::Internal(e.message().to_string()))?;

    let inner = resp.into_inner();
    if inner.success {
        Ok(Json(serde_json::json!({
            "success": true,
            "message": inner.message
        })))
    } else {
        Err(ApiError::NotFound(inner.message))
    }
}

#[utoipa::path(
    delete,
    path = "/deployments/{job_id}/delete",
    params(
        ("job_id" = String, Path, description = "Deployment Job ID")
    ),
    responses(
        (status = 200, description = "Deployment record removed"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
#[tracing::instrument(skip(state, auth), fields(job_id = %job_id))]
pub async fn delete_deployment_record(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(job_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    // 1. Try to notify scheduler (optional/best effort)
    let channel_res = crate::scheduler::connect(&state.scheduler_config).await;

    if let Ok(channel) = channel_res {
        let mut client = mikrom_proto::scheduler::SchedulerServiceClient::new(channel);
        let req = mikrom_proto::scheduler::DeleteAppRequest {
            job_id: job_id.clone(),
            user_id: auth.user_id,
        };

        // We ignore the result of the scheduler delete, we just try our best
        let _ = client.delete_app(req).await;
    } else {
        tracing::warn!(job_id = %job_id, "Scheduler unreachable during deletion, removing from DB only");
    }

    // 2. Always delete from database
    state
        .app_repo
        .delete_deployment_by_job_id(&job_id)
        .await
        .map_err(|e| {
            ApiError::Internal(format!("Failed to remove deployment from database: {}", e))
        })?;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Deployment record removed from Mikrom"
    })))
}

#[utoipa::path(
    post,
    path = "/deployments/{job_id}/pause",
    params(
        ("job_id" = String, Path, description = "Deployment Job ID")
    ),
    responses(
        (status = 200, description = "Deployment paused"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
#[tracing::instrument(skip(state, auth), fields(job_id = %job_id))]
pub async fn pause_deployment(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(job_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let channel = crate::scheduler::connect(&state.scheduler_config)
        .await
        .map_err(ApiError::Scheduler)?;

    let mut client = mikrom_proto::scheduler::SchedulerServiceClient::new(channel);
    let req = mikrom_proto::scheduler::PauseRequest {
        job_id,
        user_id: auth.user_id,
    };

    let resp = client.pause_app(req).await.map_err(map_grpc_error)?;

    let inner = resp.into_inner();
    if inner.success {
        Ok(Json(
            serde_json::json!({ "success": true, "message": inner.message }),
        ))
    } else {
        Err(ApiError::BadRequest(inner.message))
    }
}

#[utoipa::path(
    post,
    path = "/deployments/{job_id}/resume",
    params(
        ("job_id" = String, Path, description = "Deployment Job ID")
    ),
    responses(
        (status = 200, description = "Deployment resumed"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
#[tracing::instrument(skip(state, auth), fields(job_id = %job_id))]
pub async fn resume_deployment(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(job_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let channel = crate::scheduler::connect(&state.scheduler_config)
        .await
        .map_err(ApiError::Scheduler)?;

    let mut client = mikrom_proto::scheduler::SchedulerServiceClient::new(channel);
    let req = mikrom_proto::scheduler::ResumeRequest {
        job_id,
        user_id: auth.user_id,
    };

    let resp = client.resume_app(req).await.map_err(map_grpc_error)?;

    let inner = resp.into_inner();
    if inner.success {
        Ok(Json(
            serde_json::json!({ "success": true, "message": inner.message }),
        ))
    } else {
        Err(ApiError::BadRequest(inner.message))
    }
}

fn map_grpc_error(e: tonic::Status) -> ApiError {
    match e.code() {
        tonic::Code::NotFound => ApiError::NotFound(e.message().to_string()),
        tonic::Code::InvalidArgument => ApiError::BadRequest(e.message().to_string()),
        tonic::Code::Unavailable => ApiError::Scheduler("Scheduler unavailable".to_string()),
        _ => ApiError::Internal(e.message().to_string()),
    }
}
