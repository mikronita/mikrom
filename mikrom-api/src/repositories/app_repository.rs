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
}

#[derive(Debug, Clone, Default)]
pub struct UpdateDeploymentParams {
    pub status: Option<String>,
    pub job_id: Option<String>,
    pub image_tag: Option<String>,
    pub build_id: Option<String>,
    pub ip_address: Option<String>,
    pub git_commit_hash: Option<String>,
    pub git_commit_message: Option<String>,
    pub git_branch: Option<String>,
}

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
}

#[mockall::automock]
#[async_trait]
pub trait AppRepository: Send + Sync {
    async fn create_app(&self, params: CreateAppParams) -> anyhow::Result<App>;
    async fn get_app(&self, id: Uuid) -> anyhow::Result<Option<App>>;
    async fn get_app_by_name(&self, name: &str) -> anyhow::Result<Option<App>>;
    async fn delete_app(&self, id: Uuid) -> anyhow::Result<()>;
    async fn list_apps_by_user(&self, user_id: Option<Uuid>) -> anyhow::Result<Vec<App>>;
    async fn set_active_deployment(&self, app_id: Uuid, deployment_id: Uuid) -> anyhow::Result<()>;
    async fn update_app_port(&self, id: Uuid, port: i32) -> anyhow::Result<()>;

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
}
