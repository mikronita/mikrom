pub mod orchestrator;
pub mod service;
pub mod worker;
pub mod workflow;

pub use orchestrator::DeploymentOrchestrator;
pub use service::{DeployParams, DeploymentService, TriggerBuildParams};
pub use worker::{BuildTask, start_build_polling};
pub use workflow::DeploymentPromotionWorkflow;

use crate::domain::types::{CpuCores, MemoryMb, Port};

pub const DEFAULT_DEPLOYMENT_VCPUS: u32 = 1;
pub const DEFAULT_DEPLOYMENT_MEMORY_MIB: u32 = 512;
pub const DEPLOYMENT_VCPU_OPTIONS: [u32; 4] = [1, 2, 3, 4];
pub const DEPLOYMENT_MEMORY_OPTIONS_MIB: [u32; 4] = [512, 1024, 2048, 4096];

pub fn resolve_deployment_vcpus(vcpus: Option<CpuCores>) -> crate::error::ApiResult<CpuCores> {
    let value = vcpus.unwrap_or_else(|| CpuCores::new(DEFAULT_DEPLOYMENT_VCPUS).unwrap());
    if DEPLOYMENT_VCPU_OPTIONS.contains(&value.value()) {
        Ok(value)
    } else {
        Err(crate::error::ApiError::BadRequest(format!(
            "Unsupported CPU value {}. Allowed values are 1, 2, 3, and 4.",
            value.value()
        )))
    }
}

pub fn resolve_deployment_memory_mib(
    memory_mib: Option<MemoryMb>,
) -> crate::error::ApiResult<MemoryMb> {
    let value = memory_mib.unwrap_or_else(|| MemoryMb::new(DEFAULT_DEPLOYMENT_MEMORY_MIB).unwrap());
    if DEPLOYMENT_MEMORY_OPTIONS_MIB.contains(&value.value()) {
        Ok(value)
    } else {
        Err(crate::error::ApiError::BadRequest(format!(
            "Unsupported memory value {}. Allowed values are 512, 1024, 2048, and 4096 MiB.",
            value.value()
        )))
    }
}

pub fn resolve_deployment_hypervisor(hypervisor: Option<&str>) -> i32 {
    match hypervisor {
        Some("firecracker") => mikrom_proto::scheduler::HypervisorType::HypertypeFirecracker as i32,
        Some("qemu") => mikrom_proto::scheduler::HypervisorType::HypertypeQemuMicrovm as i32,
        _ => mikrom_proto::scheduler::HypervisorType::HypertypeUnspecified as i32,
    }
}

#[derive(Debug, serde::Serialize, rovo::schemars::JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum AppScaleState {
    Active,
    Idle,
    ScaledToZero,
    WarmingUp,
}

pub async fn resolve_app_scale_state(
    state: &crate::AppState,
    app: &crate::domain::App,
) -> AppScaleState {
    // Check if there are any running replicas for this app in the scheduler
    let jobs = state
        .scheduler
        .list_apps(mikrom_proto::scheduler::ListAppsRequest {
            user_id: app.user_id.to_string(),
            status: Some(mikrom_proto::scheduler::DeployStatus::Running as i32),
        })
        .await;

    match jobs {
        Ok(resp) => {
            let running_count = resp
                .apps
                .iter()
                .filter(|j| j.app_id == app.id.to_string())
                .count();

            resolve_app_scale_state_from_running_count(
                app,
                running_count,
                chrono::Utc::now().timestamp(),
            )
        },
        Err(_) => AppScaleState::ScaledToZero,
    }
}

pub fn resolve_app_scale_state_from_running_count(
    app: &crate::domain::App,
    running_count: usize,
    now: i64,
) -> AppScaleState {
    if running_count > 0 {
        // If traffic in the last 10 seconds, mark as active, otherwise idle
        if app.last_router_traffic_at > 0 && now - app.last_router_traffic_at < 10 {
            AppScaleState::Active
        } else {
            AppScaleState::Idle
        }
    } else if app.desired_replicas > 0 && app.active_deployment_id.is_some() {
        // If we have a deployment and desired replicas > 0, we're warming up
        AppScaleState::WarmingUp
    } else {
        AppScaleState::ScaledToZero
    }
}

#[derive(Debug, serde::Serialize, rovo::schemars::JsonSchema)]
pub struct AppResponse {
    pub id: uuid::Uuid,
    pub name: String,
    pub git_url: String,
    pub port: Port,
    pub hostname: Option<String>,
    pub github_webhook_secret: Option<String>,
    pub github_installation_id: Option<i64>,
    pub github_repo_id: Option<i64>,
    pub github_repo_full_name: Option<String>,
    pub active_deployment_id: Option<uuid::Uuid>,
    pub health_check_path: String,
    pub drain_timeout: i32,
    pub desired_replicas: i32,
    pub min_replicas: i32,
    pub max_replicas: i32,
    pub autoscaling_enabled: bool,
    pub cpu_threshold: f64,
    pub mem_threshold: f64,
    pub scale_state: AppScaleState,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub fn build_app_response_with_scale_state(
    app: &crate::domain::App,
    scale_state: AppScaleState,
) -> AppResponse {
    AppResponse {
        id: app.id,
        name: app.name.clone(),
        git_url: app.git_url.clone(),
        port: app.port,
        hostname: app.hostname.clone(),
        github_webhook_secret: app.github_webhook_secret.clone(),
        github_installation_id: app.github_installation_id,
        github_repo_id: app.github_repo_id,
        github_repo_full_name: app.github_repo_full_name.clone(),
        active_deployment_id: app.active_deployment_id,
        health_check_path: app.health_check_path.clone(),
        drain_timeout: app.drain_timeout,
        desired_replicas: app.desired_replicas,
        min_replicas: app.min_replicas,
        max_replicas: app.max_replicas,
        autoscaling_enabled: app.autoscaling_enabled,
        cpu_threshold: app.cpu_threshold,
        mem_threshold: app.mem_threshold,
        scale_state,
        created_at: app.created_at,
    }
}

pub async fn build_app_response(state: &crate::AppState, app: &crate::domain::App) -> AppResponse {
    let scale_state = resolve_app_scale_state(state, app).await;

    build_app_response_with_scale_state(app, scale_state)
}

#[derive(Debug, serde::Deserialize, rovo::schemars::JsonSchema, Clone)]
pub struct DeployRequestPayload {
    pub app_name: String,
    pub image: String,
    pub git_url: Option<String>,
    pub port: Option<Port>,
    pub vcpus: Option<CpuCores>,
    pub memory_mib: Option<MemoryMb>,
    pub disk_mib: Option<u32>,
    pub env: Option<std::collections::HashMap<String, String>>,
    pub hypervisor: Option<String>,
}

#[derive(Debug, serde::Serialize, rovo::schemars::JsonSchema)]
pub struct DeployResponseBody {
    pub job_id: Option<String>,
    pub deployment_id: Option<String>,
    pub status: String,
    pub host_id: Option<String>,
    pub vm_id: Option<String>,
    pub image_tag: Option<String>,
    pub message: String,
}
