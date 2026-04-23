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

#[derive(Debug, Serialize)]
pub struct VmInfo {
    pub job_id: String,
    pub app_id: String,
    pub app_name: String,
    pub image: String,
    pub status: String,
    pub host_id: String,
    pub vm_id: String,
}

#[derive(Debug, Serialize)]
pub struct VmStatusResponse {
    pub job_id: String,
    pub status: String,
    pub host_id: String,
    pub vm_id: String,
    pub scheduled_at: i64,
    pub started_at: i64,
    pub stopped_at: i64,
    pub error_message: String,
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
}

#[tracing::instrument(skip(state, auth))]
pub async fn list_vms(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
) -> ApiResult<Json<Vec<VmInfo>>> {
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

    // 3. Map deployments to VmInfo, using scheduler data if available
    let mut vms = Vec::new();
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

            vms.push(VmInfo {
                job_id: job_id.clone(),
                app_id: dep.app_id.to_string(),
                app_name,
                image: dep.image_tag.unwrap_or_default(),
                status,
                host_id,
                vm_id,
            });
        }
    }

    Ok(Json(vms))
}

#[tracing::instrument(skip(state, auth), fields(job_id = %job_id))]
pub async fn get_vm_status(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(job_id): Path<String>,
) -> ApiResult<Json<VmStatusResponse>> {
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
    let vm = VmStatusResponse {
        job_id: inner.job_id,
        status: crate::scheduler::status_name(inner.status).to_string(),
        host_id: inner.host_id,
        vm_id: inner.vm_id,
        scheduled_at: inner.scheduled_at,
        started_at: inner.started_at,
        stopped_at: inner.stopped_at,
        error_message: inner.error_message,
        cpu_usage: inner.cpu_usage,
        ram_used_bytes: inner.ram_used_bytes,
    };

    Ok(Json(vm))
}

#[tracing::instrument(skip(state, auth), fields(job_id = %job_id))]
pub async fn get_vm_logs(
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

#[tracing::instrument(skip(state, auth), fields(job_id = %job_id))]
pub async fn stop_vm(
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

#[tracing::instrument(skip(state, auth), fields(job_id = %job_id))]
pub async fn delete_vm(
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

#[tracing::instrument(skip(state, auth), fields(job_id = %job_id))]
pub async fn pause_vm(
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

#[tracing::instrument(skip(state, auth), fields(job_id = %job_id))]
pub async fn resume_vm(
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
        tonic::Code::PermissionDenied => ApiError::Forbidden,
        tonic::Code::FailedPrecondition | tonic::Code::InvalidArgument => {
            ApiError::BadRequest(e.message().to_string())
        },
        tonic::Code::Unavailable => ApiError::Scheduler("Scheduler unavailable".to_string()),
        _ => ApiError::Internal(e.message().to_string()),
    }
}

// Tests ...
