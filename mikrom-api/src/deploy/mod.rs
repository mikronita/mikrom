use crate::error::{ApiError, ApiResult};
use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod handlers;
pub use handlers::*;

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
    pub git_url: Option<String>,
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
    pub image_tag: Option<String>,
    pub message: String,
}

#[tracing::instrument(skip(state, auth, payload), fields(app_name = %payload.app_name, image = %payload.image))]
pub async fn deploy_app(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Json(payload): Json<DeployRequestBody>,
) -> ApiResult<Json<DeployResponseBody>> {
    let mut final_image = payload.image.clone();

    // If git_url is provided, trigger the builder
    if let Some(git_url) = &payload.git_url {
        tracing::info!(git_url = %git_url, "Triggering build for Git repository");

        let builder_channel = crate::builder::connect(&state.builder_addr)
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to connect to builder: {}", e)))?;

        let mut builder_client = mikrom_proto::builder::BuilderServiceClient::new(builder_channel);

        let build_req = mikrom_proto::builder::BuildRequest {
            app_id: Uuid::new_v4().to_string(),
            git_url: git_url.clone(),
            image_name: payload.app_name.to_lowercase().replace(" ", "-"),
            tag: "latest".to_string(),
        };

        let build_resp = builder_client
            .build_app(build_req)
            .await
            .map_err(|e| ApiError::Internal(format!("Build initiation failed: {}", e)))?
            .into_inner();

        let build_id = build_resp.build_id;
        tracing::info!(build_id = %build_id, "Build initiated, polling for status");

        // Simple polling loop
        let mut attempts = 0;
        loop {
            if attempts > 60 {
                // 5 minutes timeout (5s * 60)
                return Err(ApiError::Internal("Build timed out".to_string()));
            }

            let status_resp = builder_client
                .get_build_status(mikrom_proto::builder::GetBuildStatusRequest {
                    build_id: build_id.clone(),
                })
                .await
                .map_err(|e| ApiError::Internal(format!("Failed to get build status: {}", e)))?
                .into_inner();

            match mikrom_proto::builder::BuildStatus::try_from(status_resp.status)
                .unwrap_or(mikrom_proto::builder::BuildStatus::Unspecified)
            {
                mikrom_proto::builder::BuildStatus::Success => {
                    final_image = status_resp.image_tag;
                    tracing::info!(image_tag = %final_image, "Build successful");
                    break;
                }
                mikrom_proto::builder::BuildStatus::Failed => {
                    return Err(ApiError::Internal(format!(
                        "Build failed: {}",
                        status_resp.message
                    )));
                }
                _ => {
                    attempts += 1;
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    }

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
        image: final_image.clone(),
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
        image_tag: Some(final_image),
        message: inner.message,
    };

    tracing::info!(job_id = %result.job_id, status = %result.status, "Deployment processed");

    Ok(Json(result))
}
