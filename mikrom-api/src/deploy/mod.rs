use crate::error::{ApiError, ApiResult};
use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

pub mod handlers;
pub mod webhooks;
pub mod worker;
pub use handlers::*;

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeployRequestPayload {
    pub app_name: String,
    pub image: String,
    pub git_url: Option<String>,
    pub vcpus: Option<u32>,
    pub memory_mib: Option<u32>,
    pub disk_mib: Option<u32>,
    pub port: Option<u32>,
    pub env: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeployResponseBody {
    pub job_id: Option<String>,
    pub deployment_id: Option<String>,
    pub status: String,
    pub host_id: Option<String>,
    pub vm_id: Option<String>,
    pub image_tag: Option<String>,
    pub message: String,
}

#[utoipa::path(
    post,
    path = "/deploy",
    request_body = DeployRequestPayload,
    responses(
        (status = 200, description = "Deployment initiated", body = DeployResponseBody),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 500, description = "Internal error", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
pub async fn deploy_app(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Json(payload): Json<DeployRequestPayload>,
) -> ApiResult<Json<DeployResponseBody>> {
    let final_image = payload.image.clone();

    let vcpus = payload.vcpus.unwrap_or(1);
    let memory_mib = payload.memory_mib.unwrap_or(256);
    let disk_mib = payload.disk_mib.unwrap_or(1024);
    let port = payload.port.unwrap_or(8080);

    // If git_url is provided, trigger the builder in background
    if let Some(git_url) = &payload.git_url {
        tracing::info!(git_url = %git_url, "Triggering build for Git repository via NATS");

        use prost::Message;
        let app_id = Uuid::new_v4();
        let build_req = mikrom_proto::builder::BuildRequest {
            app_id: app_id.to_string(),
            git_url: git_url.clone(),
            image_name: payload.app_name.to_lowercase().replace(' ', "-"),
            tag: "latest".to_string(),
        };

        let mut buf = Vec::new();
        build_req
            .encode(&mut buf)
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        let response = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            state
                .nats_client
                .request("mikrom.builder.build", buf.into()),
        )
        .await
        .map_err(|_| ApiError::Internal("Builder request timed out".into()))?
        .map_err(|e| ApiError::Internal(format!("Failed to trigger build via NATS: {}", e)))?;

        let build_resp = mikrom_proto::builder::BuildResponse::decode(&response.payload[..])
            .map_err(|e| ApiError::Internal(format!("Failed to decode builder response: {}", e)))?;

        let build_id = build_resp.build_id;
        tracing::info!(build_id = %build_id, "Build initiated via NATS, starting background polling");

        let task = crate::deploy::worker::BuildTask {
            deployment_id: Uuid::new_v4(), // Dummy for /deploy endpoint
            app_id,
            app_name: payload.app_name.clone(),
            user_id: auth.user_id.clone(),
            build_id: build_id.clone(),
            vcpus,
            memory_mib: memory_mib as u64,
            disk_mib: disk_mib as u64,
            port,
            env: payload.env.clone().unwrap_or_default(),
        };

        crate::deploy::worker::start_build_polling(state.clone(), task).await;

        return Ok(Json(DeployResponseBody {
            job_id: None,
            deployment_id: None,
            status: "BUILDING".to_string(),
            host_id: None,
            vm_id: None,
            image_tag: None,
            message: "Build triggered and polling started".to_string(),
        }));
    }

    let nats_req = mikrom_proto::scheduler::DeployRequest {
        app_id: Uuid::new_v4().to_string(),
        app_name: payload.app_name.clone(),
        image: final_image.clone(),
        user_id: auth.user_id,
        config: Some(mikrom_proto::scheduler::AppConfig {
            vcpus,
            memory_mib,
            disk_mib,
            port,
            env: payload.env.clone().unwrap_or_default(),
            ip_address: String::new(),
            gateway: String::new(),
            mac_address: String::new(),
            volumes: vec![],
        }),
        deployment_id: String::new(), // Not applicable for one-off deploy
    };

    use prost::Message;
    let mut payload_bytes = Vec::new();
    nats_req
        .encode(&mut payload_bytes)
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    tracing::info!("Sending deployment request via NATS (Protobuf)...");
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        state
            .nats_client
            .request("mikrom.scheduler.deploy", payload_bytes.into()),
    )
    .await
    .map_err(|_| ApiError::Internal("Scheduler request timed out".into()))?
    .map_err(|e| ApiError::Internal(format!("NATS request failed: {}", e)))?;

    let inner = mikrom_proto::scheduler::DeployResponse::decode(&response.payload[..])
        .map_err(|e| ApiError::Internal(format!("Failed to decode NATS response: {}", e)))?;

    let result = DeployResponseBody {
        job_id: Some(inner.job_id),
        deployment_id: None,
        status: crate::scheduler::status_name(inner.status).to_string(),
        host_id: Some(inner.host_id),
        vm_id: Some(inner.vm_id),
        image_tag: Some(final_image),
        message: inner.message,
    };

    tracing::info!(job_id = ?result.job_id, status = %result.status, "Deployment processed");

    Ok(Json(result))
}
