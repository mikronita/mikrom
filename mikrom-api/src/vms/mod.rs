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
    let channel = crate::scheduler::connect(&state.scheduler_config)
        .await
        .map_err(ApiError::Scheduler)?;

    let mut client = mikrom_proto::scheduler::SchedulerServiceClient::new(channel);
    let req = mikrom_proto::scheduler::ListAppsRequest {
        user_id: auth.user_id,
        status: None,
    };

    let resp = client
        .list_apps(req)
        .await
        .map_err(|e| ApiError::Internal(e.message().to_string()))?;

    let vms: Vec<VmInfo> = resp
        .into_inner()
        .apps
        .into_iter()
        .map(|a| VmInfo {
            job_id: a.job_id,
            app_id: a.app_id,
            app_name: a.app_name,
            image: a.image,
            status: crate::scheduler::status_name(a.status).to_string(),
            host_id: a.host_id,
            vm_id: a.vm_id,
        })
        .collect();

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
        cpu_usage: 0.0,
        ram_used_bytes: 0,
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
        }
        Err(e) => {
            let data = serde_json::json!({
                "line": format!("Error: {}", e),
                "timestamp": chrono::Utc::now().timestamp(),
            })
            .to_string();
            Ok::<Event, std::convert::Infallible>(Event::default().data(data))
        }
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
    let channel = crate::scheduler::connect(&state.scheduler_config)
        .await
        .map_err(ApiError::Scheduler)?;

    let mut client = mikrom_proto::scheduler::SchedulerServiceClient::new(channel);
    let req = mikrom_proto::scheduler::DeleteAppRequest {
        job_id,
        user_id: auth.user_id,
    };

    let resp = client
        .delete_app(req)
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
        }
        tonic::Code::Unavailable => ApiError::Scheduler("Scheduler unavailable".to_string()),
        _ => ApiError::Internal(e.message().to_string()),
    }
}

// Tests ...
