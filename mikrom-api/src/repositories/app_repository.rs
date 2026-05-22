use crate::models::app::{App, Deployment};
use async_trait::async_trait;
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
pub struct NewDeployment {
    pub app_id: Uuid,
    pub user_id: String,
    pub vcpus: i32,
    pub memory_mib: i64,
    pub disk_mib: i64,
    pub port: i32,
    pub env_vars: std::collections::HashMap<String, String>,
    pub trigger_source: String,
    pub git_commit_hash: Option<String>,
    pub git_commit_message: Option<String>,
    pub git_branch: Option<String>,
    pub hypervisor: i32,
}

impl NewDeployment {
    pub fn from_handler(
        app_id: Uuid,
        user_id: String,
        vcpus: i32,
        memory_mib: i64,
        disk_mib: i64,
        port: i32,
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
    pub port: i32,
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
    async fn create_app(&self, params: CreateAppParams) -> anyhow::Result<App>;
    async fn get_app(&self, id: Uuid) -> anyhow::Result<Option<App>>;
    async fn get_app_by_name(&self, name: &str) -> anyhow::Result<Option<App>>;
    async fn get_app_by_github_repo_id(&self, repo_id: i64) -> anyhow::Result<Option<App>>;
    async fn delete_app(&self, id: Uuid) -> anyhow::Result<()>;
    async fn list_apps_by_user(&self, user_id: Option<Uuid>) -> anyhow::Result<Vec<App>>;
    async fn set_active_deployment(&self, app_id: Uuid, deployment_id: Uuid) -> anyhow::Result<()>;
    async fn update_app_port(&self, id: Uuid, port: i32) -> anyhow::Result<()>;
    async fn update_app_scaling(&self, id: Uuid, desired_replicas: i32) -> anyhow::Result<()>;
    async fn update_app_autoscaling(
        &self,
        id: Uuid,
        min_replicas: i32,
        max_replicas: i32,
        enabled: bool,
        cpu_threshold: Option<f64>,
        mem_threshold: Option<f64>,
    ) -> anyhow::Result<()>;

    async fn create_deployment(&self, data: NewDeployment) -> anyhow::Result<Deployment>;
    async fn update_deployment(
        &self,
        id: Uuid,
        params: UpdateDeploymentParams,
    ) -> anyhow::Result<()>;
    async fn update_deployment_port(&self, id: Uuid, port: i32) -> anyhow::Result<()>;
    async fn get_deployment(&self, id: Uuid) -> anyhow::Result<Option<Deployment>>;
    async fn get_deployment_by_job_id(&self, job_id: &str) -> anyhow::Result<Option<Deployment>>;
    async fn list_deployments_by_app(&self, app_id: Uuid) -> anyhow::Result<Vec<Deployment>>;
    async fn list_deployments_by_user(
        &self,
        user_id: Option<Uuid>,
    ) -> anyhow::Result<Vec<Deployment>>;
    async fn get_active_deployment(&self, app_id: Uuid) -> anyhow::Result<Option<Deployment>>;
    async fn delete_deployment_by_job_id(&self, job_id: &str) -> anyhow::Result<()>;

    async fn list_security_rules(
        &self,
        app_id: Uuid,
    ) -> anyhow::Result<Vec<crate::models::app::SecurityRule>>;
    async fn create_security_rule(
        &self,
        app_id: Uuid,
        protocol: String,
        port_start: i32,
        port_end: i32,
        action: String,
    ) -> anyhow::Result<crate::models::app::SecurityRule>;
    async fn delete_security_rule(&self, id: Uuid) -> anyhow::Result<()>;
}
