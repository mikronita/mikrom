use crate::domain::error::DomainResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone, rovo::schemars::JsonSchema)]
pub struct App {
    pub id: Uuid,
    pub name: String,
    pub git_url: String,
    pub port: u32,
    pub hostname: Option<String>,
    pub user_id: Uuid,
    pub github_webhook_secret: Option<String>,
    pub github_installation_id: Option<i64>,
    pub github_repo_id: Option<i64>,
    pub github_repo_full_name: Option<String>,
    pub active_deployment_id: Option<Uuid>,
    pub health_check_path: String,
    pub drain_timeout: i32,
    pub desired_replicas: i32,
    pub min_replicas: i32,
    pub max_replicas: i32,
    pub autoscaling_enabled: bool,
    pub cpu_threshold: f64,
    pub mem_threshold: f64,
    pub last_router_traffic_at: i64,
    pub last_scaled_to_zero_at: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: String::new(),
            git_url: String::new(),
            port: 8080,
            hostname: None,
            user_id: Uuid::new_v4(),
            github_webhook_secret: None,
            github_installation_id: None,
            github_repo_id: None,
            github_repo_full_name: None,
            active_deployment_id: None,
            health_check_path: "/".to_string(),
            drain_timeout: 10,
            desired_replicas: 1,
            min_replicas: 0,
            max_replicas: 1,
            autoscaling_enabled: false,
            cpu_threshold: 80.0,
            mem_threshold: 80.0,
            last_router_traffic_at: 0,
            last_scaled_to_zero_at: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, rovo::schemars::JsonSchema, Default)]
pub struct Deployment {
    pub id: Uuid,
    pub app_id: Uuid,
    pub user_id: Uuid,
    pub build_id: Option<String>,
    pub image_tag: Option<String>,
    pub job_id: Option<String>,
    pub ipv6_address: Option<String>,
    pub status: String,
    pub vcpus: i32,
    pub memory_mib: i64,
    pub disk_mib: i64,
    pub port: u32,
    pub env_vars: serde_json::Value,
    pub git_commit_hash: Option<String>,
    pub git_commit_message: Option<String>,
    pub git_branch: Option<String>,
    pub trigger_source: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub hypervisor: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone, rovo::schemars::JsonSchema)]
pub struct SecurityRule {
    pub id: Uuid,
    pub app_id: Uuid,
    pub protocol: String,
    pub port_start: u32,
    pub port_end: u32,
    pub action: String,
    pub priority: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct NewDeployment {
    pub app_id: Uuid,
    pub user_id: String,
    pub vcpus: i32,
    pub memory_mib: i64,
    pub disk_mib: i64,
    pub port: u32,
    pub env_vars: std::collections::HashMap<String, String>,
    pub trigger_source: String,
    pub git_commit_hash: Option<String>,
    pub git_commit_message: Option<String>,
    pub git_branch: Option<String>,
    pub hypervisor: i32,
}

impl NewDeployment {
    #[allow(clippy::too_many_arguments)]
    pub fn from_handler(
        app_id: Uuid,
        user_id: String,
        vcpus: i32,
        memory_mib: i64,
        disk_mib: i64,
        port: u32,
        env_vars: std::collections::HashMap<String, String>,
        trigger_source: String,
        git_metadata: Option<&GitMetadata>,
        hypervisor: i32,
    ) -> Self {
        Self {
            app_id,
            user_id,
            vcpus,
            memory_mib,
            disk_mib,
            port,
            env_vars,
            trigger_source,
            git_commit_hash: git_metadata.and_then(|m| m.git_commit_hash.clone()),
            git_commit_message: git_metadata.and_then(|m| m.git_commit_message.clone()),
            git_branch: git_metadata.and_then(|m| m.git_branch.clone()),
            hypervisor,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct UpdateDeploymentParams {
    pub status: Option<String>,
    pub job_id: Option<String>,
    pub image_tag: Option<String>,
    pub build_id: Option<String>,
    pub ipv6_address: Option<String>,
    pub git_commit_hash: Option<String>,
    pub git_commit_message: Option<String>,
    pub git_branch: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, rovo::schemars::JsonSchema)]
pub struct GitMetadata {
    pub git_commit_hash: Option<String>,
    pub git_commit_message: Option<String>,
    pub git_branch: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CreateAppParams {
    pub name: String,
    pub git_url: String,
    pub port: u32,
    pub hostname: Option<String>,
    pub user_id: Uuid,
    pub github_webhook_secret: Option<String>,
    pub github_installation_id: Option<i64>,
    pub github_repo_id: Option<i64>,
    pub github_repo_full_name: Option<String>,
    pub health_check_path: Option<String>,
    pub drain_timeout: Option<i32>,
    pub desired_replicas: Option<i32>,
    pub min_replicas: Option<i32>,
    pub max_replicas: Option<i32>,
    pub autoscaling_enabled: Option<bool>,
    pub cpu_threshold: Option<f64>,
    pub mem_threshold: Option<f64>,
}

impl Default for CreateAppParams {
    fn default() -> Self {
        Self {
            name: String::new(),
            git_url: String::new(),
            port: 8080,
            hostname: None,
            user_id: Uuid::new_v4(),
            github_webhook_secret: None,
            github_installation_id: None,
            github_repo_id: None,
            github_repo_full_name: None,
            health_check_path: None,
            drain_timeout: None,
            desired_replicas: None,
            min_replicas: None,
            max_replicas: None,
            autoscaling_enabled: None,
            cpu_threshold: None,
            mem_threshold: None,
        }
    }
}

#[mockall::automock]
#[async_trait]
pub trait AppRepository: Send + Sync {
    async fn create_app(&self, params: CreateAppParams) -> DomainResult<App>;
    async fn get_app(&self, id: Uuid) -> DomainResult<Option<App>>;
    async fn get_app_by_name(&self, name: &str) -> DomainResult<Option<App>>;
    async fn get_app_by_github_repo_id(&self, repo_id: i64) -> DomainResult<Option<App>>;
    async fn delete_app(&self, id: Uuid) -> DomainResult<()>;
    async fn list_apps_by_user(&self, user_id: Option<Uuid>) -> DomainResult<Vec<App>>;
    async fn set_active_deployment(&self, app_id: Uuid, deployment_id: Uuid) -> DomainResult<()>;
    async fn update_app_port(&self, id: Uuid, port: u32) -> DomainResult<()>;
    async fn update_app_scaling(&self, id: Uuid, desired_replicas: i32) -> DomainResult<()>;
    async fn update_app_autoscaling(
        &self,
        id: Uuid,
        min_replicas: i32,
        max_replicas: i32,
        enabled: bool,
        cpu_threshold: Option<f64>,
        mem_threshold: Option<f64>,
    ) -> DomainResult<()>;

    async fn create_deployment(&self, data: NewDeployment) -> DomainResult<Deployment>;
    async fn update_deployment(&self, id: Uuid, params: UpdateDeploymentParams)
    -> DomainResult<()>;
    async fn update_deployment_port(&self, id: Uuid, port: u32) -> DomainResult<()>;
    async fn get_deployment(&self, id: Uuid) -> DomainResult<Option<Deployment>>;
    async fn get_deployment_by_job_id(&self, job_id: &str) -> DomainResult<Option<Deployment>>;
    async fn list_deployments_by_app(&self, app_id: Uuid) -> DomainResult<Vec<Deployment>>;
    async fn list_deployments_by_user(
        &self,
        user_id: Option<Uuid>,
    ) -> DomainResult<Vec<Deployment>>;
    async fn get_active_deployment(&self, app_id: Uuid) -> DomainResult<Option<Deployment>>;
    async fn delete_deployment_by_job_id(&self, job_id: &str) -> DomainResult<()>;

    async fn list_security_rules(&self, app_id: Uuid) -> DomainResult<Vec<SecurityRule>>;
    async fn create_security_rule(
        &self,
        app_id: Uuid,
        protocol: String,
        port_start: u32,
        port_end: u32,
        action: String,
    ) -> DomainResult<SecurityRule>;
    async fn delete_security_rule(&self, id: Uuid) -> DomainResult<()>;
}
