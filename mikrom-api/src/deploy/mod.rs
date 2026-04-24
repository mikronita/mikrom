use crate::error::{ApiError, ApiResult};
use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

pub mod handlers;
pub mod webhooks;
pub mod worker;
pub use handlers::*;
pub use worker::*;

#[derive(Debug, Deserialize, ToSchema)]
pub struct VolumeRequest {
    pub volume_id: String,
    pub size_mib: u64,
    pub read_only: Option<bool>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeployRequestBody {
    pub app_name: String,
    pub image: String,
    pub git_url: Option<String>,
    pub port: Option<i32>,
    pub vcpus: Option<u32>,
    pub memory_mib: Option<u64>,
    pub disk_mib: Option<u64>,
    pub env: Option<std::collections::HashMap<String, String>>,
    pub volumes: Option<Vec<VolumeRequest>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeployResponseBody {
    pub job_id: Option<String>,
    pub deployment_id: Option<Uuid>,
    pub status: String,
    pub host_id: Option<String>,
    pub vm_id: Option<String>,
    pub image_tag: Option<String>,
    pub message: String,
}

#[utoipa::path(
    post,
    path = "/deploy",
    request_body = DeployRequestBody,
    responses(
        (status = 200, description = "Deployment initiated", body = DeployResponseBody),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 500, description = "Internal server error", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
#[tracing::instrument(skip(state, auth, payload), fields(app_name = %payload.app_name, image = %payload.image))]
pub async fn deploy_app(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Json(payload): Json<DeployRequestBody>,
) -> ApiResult<Json<DeployResponseBody>> {
    let final_image = payload.image.clone();

    // If git_url is provided, trigger the builder in background
    if let Some(git_url) = &payload.git_url {
        tracing::info!(git_url = %git_url, "Triggering build for Git repository");

        let builder_channel = crate::builder::connect(&state.builder_addr)
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to connect to builder: {}", e)))?;

        let mut builder_client = mikrom_proto::builder::BuilderServiceClient::new(builder_channel);

        let app_id = Uuid::new_v4();
        let build_req = mikrom_proto::builder::BuildRequest {
            app_id: app_id.to_string(),
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
        tracing::info!(build_id = %build_id, "Build initiated, starting background polling");

        // Note: For /deploy (one-off), we might not have a deployment record in DB yet
        // but we should probably create one if we want to be consistent.
        // For now, we'll return BUILDING status.

        // However, to use the background worker robustly, it needs a deployment record.
        // If this is a one-off deploy, we can either:
        // 1. Just block (bad)
        // 2. Return BUILDING and lose track if it fails (bad)
        // 3. Create a temporary app and deployment (better)

        // Given /deploy is a legacy/one-off route, we'll keep it simple but backgrounded.
        // We'll spawn the polling task directly.

        let vcpus = payload.vcpus.unwrap_or(1);
        let memory_mib = payload.memory_mib.unwrap_or(256);
        let disk_mib = payload.disk_mib.unwrap_or(1024);
        let port = payload.port.unwrap_or(8080);

        let task = BuildTask {
            deployment_id: Uuid::new_v4(), // Dummy for one-off
            app_id,
            app_name: payload.app_name.clone(),
            user_id: auth.user_id.clone(),
            build_id,
            vcpus,
            memory_mib: memory_mib as u32,
            disk_mib: disk_mib as u32,
            port: port as u32,
            env: payload.env.clone().unwrap_or_default(),
        };

        start_build_polling(state.clone(), task).await;

        return Ok(Json(DeployResponseBody {
            job_id: None,
            deployment_id: None,
            status: "BUILDING".to_string(),
            host_id: None,
            vm_id: None,
            image_tag: Some(final_image),
            message: "Build initiated in background".to_string(),
        }));
    }

    let vcpus = payload.vcpus.unwrap_or(1);
    let memory_mib = payload.memory_mib.unwrap_or(256);
    let disk_mib = payload.disk_mib.unwrap_or(1024);
    let port = payload.port.unwrap_or(8080);

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
            port: port as u32,
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
        job_id: Some(inner.job_id),
        deployment_id: None,
        status: crate::scheduler::status_name(inner.status).to_string(),
        host_id: Some(inner.host_id).filter(|s| !s.is_empty()),
        vm_id: Some(inner.vm_id).filter(|s| !s.is_empty()),
        image_tag: Some(final_image),
        message: inner.message,
    };

    tracing::info!(job_id = ?result.job_id, status = %result.status, "Deployment processed");

    Ok(Json(result))
}
