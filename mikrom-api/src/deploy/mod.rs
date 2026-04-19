use crate::error::{ApiError, ApiResult};
use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct VolumeRequest {
    pub volume_id: String,
    pub size_mib: u64,
    pub read_only: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct DeployRequestBody {
    pub app_name: String,
    pub image: String,
    pub vcpus: Option<u32>,
    pub memory_mib: Option<u64>,
    pub disk_mib: Option<u64>,
    pub env: Option<std::collections::HashMap<String, String>>,
    pub volumes: Option<Vec<VolumeRequest>>,
}

#[derive(Debug, Serialize)]
pub struct DeployResponseBody {
    pub job_id: String,
    pub status: String,
    pub host_id: Option<String>,
    pub vm_id: Option<String>,
    pub message: String,
}

#[tracing::instrument(skip(state, auth, payload), fields(app_name = %payload.app_name, image = %payload.image))]
pub async fn deploy_app(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Json(payload): Json<DeployRequestBody>,
) -> ApiResult<Json<DeployResponseBody>> {
    let vcpus = payload.vcpus.unwrap_or(1);
    let memory_mib = payload.memory_mib.unwrap_or(256);
    let disk_mib = payload.disk_mib.unwrap_or(1024);

    let channel = crate::scheduler::connect(&state.scheduler_config)
        .await
        .map_err(ApiError::Scheduler)?;

    let mut client = mikrom_proto::scheduler::SchedulerServiceClient::new(channel);

    let req = mikrom_proto::scheduler::DeployRequest {
        app_id: Uuid::new_v4().to_string(),
        app_name: payload.app_name.clone(),
        image: payload.image.clone(),
        config: Some(mikrom_proto::scheduler::AppConfig {
            vcpus,
            memory_mib: memory_mib as u32,
            disk_mib: disk_mib as u32,
            env: payload.env.clone().unwrap_or_default(),
            ip_address: String::new(),
            gateway: String::new(),
            mac_address: String::new(),
            volumes: payload
                .volumes
                .as_ref()
                .unwrap_or(&vec![])
                .iter()
                .map(|v| mikrom_proto::scheduler::Volume {
                    volume_id: v.volume_id.clone(),
                    size_mib: v.size_mib,
                    read_only: v.read_only.unwrap_or(false),
                })
                .collect(),
        }),
        user_id: auth.user_id,
    };

    let response = client
        .deploy_app(req)
        .await
        .map_err(|e| ApiError::Internal(e.message().to_string()))?;

    let inner = response.into_inner();

    let result = DeployResponseBody {
        job_id: inner.job_id,
        status: crate::scheduler::status_name(inner.status).to_string(),
        host_id: Some(inner.host_id).filter(|s| !s.is_empty()),
        vm_id: Some(inner.vm_id).filter(|s| !s.is_empty()),
        message: inner.message,
    };

    tracing::info!(job_id = %result.job_id, status = %result.status, "Deployment processed");

    Ok(Json(result))
}

// Tests ...
