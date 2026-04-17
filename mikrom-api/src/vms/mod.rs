use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Serialize;

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
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

pub async fn list_vms(
    auth: crate::auth::AuthUser,
    State(_state): State<crate::AppState>,
) -> impl IntoResponse {
    match crate::scheduler::connect().await {
        Ok(channel) => {
            let mut client = mikrom_proto::scheduler::SchedulerServiceClient::new(channel);
            let req = mikrom_proto::scheduler::ListAppsRequest {
                user_id: auth.user_id,
                status: None,
            };
            match client.list_apps(req).await {
                Ok(resp) => {
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
                    (StatusCode::OK, Json(vms)).into_response()
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorBody {
                        error: e.message().to_string(),
                    }),
                )
                    .into_response(),
            }
        }
        Err(msg) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorBody { error: msg }),
        )
            .into_response(),
    }
}

pub async fn get_vm_status(
    _auth: crate::auth::AuthUser,
    State(_state): State<crate::AppState>,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    match crate::scheduler::connect().await {
        Ok(channel) => {
            let mut client = mikrom_proto::scheduler::SchedulerServiceClient::new(channel);
            let req = mikrom_proto::scheduler::AppStatusRequest { job_id };
            match client.get_app_status(req).await {
                Ok(resp) => {
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
                    };
                    (StatusCode::OK, Json(vm)).into_response()
                }
                Err(e) if e.code() == tonic::Code::NotFound => (
                    StatusCode::NOT_FOUND,
                    Json(ErrorBody {
                        error: "Job not found".to_string(),
                    }),
                )
                    .into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorBody {
                        error: e.message().to_string(),
                    }),
                )
                    .into_response(),
            }
        }
        Err(msg) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorBody { error: msg }),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vm_info_serialization() {
        let vm = VmInfo {
            job_id: "job-1".to_string(),
            app_id: "app-1".to_string(),
            app_name: "my-app".to_string(),
            image: "nginx:latest".to_string(),
            status: "Scheduled".to_string(),
            host_id: "host-1".to_string(),
            vm_id: "vm-1".to_string(),
        };
        let json = serde_json::to_value(&vm).unwrap();
        assert_eq!(json["job_id"], "job-1");
        assert_eq!(json["status"], "Scheduled");
        assert_eq!(json["host_id"], "host-1");
    }

    #[test]
    fn test_vm_status_response_serialization() {
        let resp = VmStatusResponse {
            job_id: "job-2".to_string(),
            status: "Running".to_string(),
            host_id: "host-2".to_string(),
            vm_id: "vm-2".to_string(),
            scheduled_at: 1_700_000_000,
            started_at: 1_700_000_005,
            stopped_at: 0,
            error_message: String::new(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "Running");
        assert_eq!(json["scheduled_at"], 1_700_000_000_i64);
        assert_eq!(json["stopped_at"], 0);
    }

    #[test]
    fn test_vm_status_response_with_error_message() {
        let resp = VmStatusResponse {
            job_id: "job-3".to_string(),
            status: "Failed".to_string(),
            host_id: String::new(),
            vm_id: String::new(),
            scheduled_at: 0,
            started_at: 0,
            stopped_at: 0,
            error_message: "no workers available".to_string(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "Failed");
        assert_eq!(json["error_message"], "no workers available");
    }
}
