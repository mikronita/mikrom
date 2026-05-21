use crate::error::{ApiError, ApiResult};
use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod handlers;
pub mod orchestrator;
pub mod service;
pub mod webhooks;
pub mod worker;
pub mod workflow;
pub use handlers::*;
pub use orchestrator::DeploymentOrchestrator;
pub use workflow::DeploymentPromotionWorkflow;

pub const DEFAULT_DEPLOYMENT_VCPUS: u32 = 1;
pub const DEFAULT_DEPLOYMENT_MEMORY_MIB: u32 = 512;
pub const DEPLOYMENT_VCPU_OPTIONS: [u32; 4] = [1, 2, 3, 4];
pub const DEPLOYMENT_MEMORY_OPTIONS_MIB: [u32; 4] = [512, 1024, 2048, 4096];

pub(crate) fn resolve_deployment_vcpus(vcpus: Option<u32>) -> ApiResult<u32> {
    let value = vcpus.unwrap_or(DEFAULT_DEPLOYMENT_VCPUS);
    if DEPLOYMENT_VCPU_OPTIONS.contains(&value) {
        Ok(value)
    } else {
        Err(ApiError::BadRequest(format!(
            "Unsupported CPU value {value}. Allowed values are 1, 2, 3, and 4."
        )))
    }
}

pub(crate) fn resolve_deployment_memory_mib(memory_mib: Option<u32>) -> ApiResult<u32> {
    let value = memory_mib.unwrap_or(DEFAULT_DEPLOYMENT_MEMORY_MIB);
    if DEPLOYMENT_MEMORY_OPTIONS_MIB.contains(&value) {
        Ok(value)
    } else {
        Err(ApiError::BadRequest(format!(
            "Unsupported memory value {value}. Allowed values are 512, 1024, 2048, and 4096 MiB."
        )))
    }
}

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct DeployRequestPayload {
    pub app_name: String,
    pub image: String,
    pub git_url: Option<String>,
    /// CPU cores to allocate. Allowed values: 1, 2, 3, or 4.
    pub vcpus: Option<u32>,
    /// Memory to allocate in MiB. Allowed values: 512, 1024, 2048, or 4096.
    pub memory_mib: Option<u32>,
    pub disk_mib: Option<u32>,
    pub port: Option<u32>,
    pub env: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Serialize, rovo::schemars::JsonSchema)]
pub struct DeployResponseBody {
    pub job_id: Option<String>,
    pub deployment_id: Option<String>,
    pub status: String,
    pub host_id: Option<String>,
    pub vm_id: Option<String>,
    pub image_tag: Option<String>,
    pub message: String,
}

#[rovo::rovo]
pub async fn deploy_app(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Json(payload): Json<DeployRequestPayload>,
) -> ApiResult<Json<DeployResponseBody>> {
    let final_image = payload.image.clone();

    let vcpus = resolve_deployment_vcpus(payload.vcpus)?;
    let memory_mib = resolve_deployment_memory_mib(payload.memory_mib)?;
    let disk_mib = payload.disk_mib.unwrap_or(1024);
    let port = payload.port.unwrap_or(8080);

    // If git_url is provided, trigger the builder in background
    if let Some(git_url) = &payload.git_url {
        tracing::info!(git_url = %git_url, "Triggering build for Git repository via NATS");

        let app_id = Uuid::new_v4();
        let build_req = mikrom_proto::builder::BuildRequest {
            app_id: app_id.to_string(),
            git_url: git_url.clone(),
            image_name: payload.app_name.to_lowercase().replace(' ', "-"),
            tag: "latest".to_string(),
            git_auth_token: None,
        };

        let build_resp: mikrom_proto::builder::BuildResponse = state
            .nats
            .with_timeout(std::time::Duration::from_secs(5))
            .request("mikrom.builder.build", build_req)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

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

        let guard = state.try_start_flow(app_id.into());
        crate::deploy::worker::start_build_polling(state.clone(), task, guard).await;

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

    let user_id_uuid = Uuid::parse_str(&auth.user_id)
        .map_err(|_| ApiError::BadRequest("Invalid user ID format".to_string()))?;

    let user = state
        .user_repo
        .find_by_id(user_id_uuid)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

    let vpc_ipv6_prefix = user.vpc_ipv6_prefix.unwrap_or_default();

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
            volumes: vec![],
            health_check_path: "/".to_string(),
            ipv6_address: String::new(),
            ipv6_gateway: String::new(),
        }),
        deployment_id: String::new(), // Not applicable for one-off deploy
        vpc_ipv6_prefix,
    };

    tracing::info!("Sending deployment request via NATS (Protobuf)...");
    let inner: mikrom_proto::scheduler::DeployResponse = state
        .nats
        .with_timeout(std::time::Duration::from_secs(5))
        .request("mikrom.scheduler.deploy", nats_req)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_default_resources() {
        assert_eq!(
            resolve_deployment_vcpus(None).expect("default CPU"),
            DEFAULT_DEPLOYMENT_VCPUS
        );
        assert_eq!(
            resolve_deployment_memory_mib(None).expect("default memory"),
            DEFAULT_DEPLOYMENT_MEMORY_MIB
        );
    }

    #[test]
    fn accepts_supported_resources() {
        for cpu in DEPLOYMENT_VCPU_OPTIONS {
            assert_eq!(resolve_deployment_vcpus(Some(cpu)).unwrap(), cpu);
        }
        for memory in DEPLOYMENT_MEMORY_OPTIONS_MIB {
            assert_eq!(resolve_deployment_memory_mib(Some(memory)).unwrap(), memory);
        }
    }

    #[test]
    fn rejects_unsupported_resources() {
        assert!(resolve_deployment_vcpus(Some(0)).is_err());
        assert!(resolve_deployment_vcpus(Some(5)).is_err());
        assert!(resolve_deployment_memory_mib(Some(256)).is_err());
        assert!(resolve_deployment_memory_mib(Some(1536)).is_err());
    }
}
