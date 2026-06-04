use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub services: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RegisterResponse {
    pub user: RegisterUser,
    pub token: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RegisterUser {
    pub id: String,
    pub email: String,
    pub role: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub vpc_ipv6_prefix: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LoginResponse {
    pub token: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DeployResponse {
    pub job_id: Option<String>,
    pub deployment_id: Option<String>,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AppInfo {
    pub id: String,
    pub name: String,
    pub git_url: String,
    pub port: i32,
    pub hostname: Option<String>,
    pub active_deployment_id: Option<String>,
    #[serde(default)]
    pub desired_replicas: i32,
    #[serde(default)]
    pub min_replicas: i32,
    #[serde(default)]
    pub max_replicas: i32,
    #[serde(default)]
    pub autoscaling_enabled: bool,
    #[serde(default)]
    pub cpu_threshold: f64,
    #[serde(default)]
    pub mem_threshold: f64,
    pub created_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LiveDeploymentInfo {
    pub job_id: String,
    pub app_name: String,
    pub image: String,
    pub status: String,
    pub host_id: String,
    pub ipv6_address: Option<String>,
    pub hypervisor: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LiveDeploymentStatus {
    pub job_id: String,
    pub status: String,
    pub host_id: String,
    pub vm_id: String,
    pub scheduled_at: i64,
    pub started_at: i64,
    pub error_message: String,
    pub ipv6_address: Option<String>,
    pub hypervisor: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DeploymentInfo {
    pub id: String,
    pub image_tag: Option<String>,
    pub status: String,
    pub created_at: Option<String>,
    pub hypervisor: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct WhoamiResponse {
    #[serde(alias = "id")]
    pub user_id: String,
    pub email: String,
    pub role: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Volume {
    pub id: String,
    pub name: String,
    pub size_mib: i32,
    pub created_at: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct VolumeAttachmentInfo {
    pub app_id: String,
    pub app_name: String,
    pub mount_point: String,
    pub access_mode: i32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct VolumeWithAttachments {
    #[serde(flatten)]
    pub volume: Volume,
    pub attachments: Vec<VolumeAttachmentInfo>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AppVolume {
    pub app_id: String,
    pub volume_id: String,
    pub mount_point: String,
    pub access_mode: i32,
    pub created_at: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AttachedVolume {
    #[serde(flatten)]
    pub volume: Volume,
    pub mount_point: String,
    pub access_mode: i32,
}

#[derive(Debug, Serialize, Clone)]
pub struct ScaleRequest {
    pub desired_replicas: Option<i32>,
    pub min_replicas: Option<i32>,
    pub max_replicas: Option<i32>,
    pub autoscaling_enabled: Option<bool>,
    pub cpu_threshold: Option<f64>,
    pub mem_threshold: Option<f64>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct VolumeSnapshot {
    pub id: String,
    pub volume_id: String,
    pub name: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DatabaseInfo {
    pub id: String,
    pub name: String,
    pub engine: String,
    #[serde(default = "default_postgres_version")]
    pub postgres_version: u16,
    pub status: String,
    pub vcpus: u32,
    pub memory_mib: u32,
    pub disk_mib: u32,
    pub created_at: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DatabaseConnectionInfo {
    pub database_id: String,
    pub database_name: String,
    pub database_user: String,
    pub database_host: String,
    pub database_port: u16,
    pub ssh_host: String,
    pub ssh_user: String,
    pub ssh_port: u16,
    pub ssh_tunnel_command: String,
    pub psql_command: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct CreateDatabaseRequest {
    pub name: String,
    pub engine: String,
    #[serde(default = "default_postgres_version")]
    pub postgres_version: u16,
    pub vcpus: Option<u32>,
    pub memory_mib: Option<u32>,
    pub disk_mib: Option<u32>,
    pub settings: Option<HashMap<String, String>>,
}

fn default_postgres_version() -> u16 {
    16
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ProjectInfo {
    pub id: String,
    pub tenant_id: String,
    pub name: String,
    pub created_at: Option<String>,
}
