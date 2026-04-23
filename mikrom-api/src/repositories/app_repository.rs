use crate::models::app::{App, Deployment};
use async_trait::async_trait;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct NewDeployment {
    pub app_id: Uuid,
    pub user_id: String,
    pub vcpus: i32,
    pub memory_mib: i64,
    pub disk_mib: i64,
    pub port: i32,
    pub env_vars: std::collections::HashMap<String, String>,
}

#[mockall::automock]
#[async_trait]
pub trait AppRepository: Send + Sync {
    async fn create_app(
        &self,
        name: &str,
        git_url: &str,
        port: i32,
        hostname: Option<String>,
        user_id: &str,
    ) -> anyhow::Result<App>;
    async fn get_app(&self, id: Uuid) -> anyhow::Result<Option<App>>;
    async fn delete_app(&self, id: Uuid) -> anyhow::Result<()>;
    async fn list_apps_by_user(&self, user_id: &str) -> anyhow::Result<Vec<App>>;
    async fn set_active_deployment(&self, app_id: Uuid, deployment_id: Uuid) -> anyhow::Result<()>;
    async fn update_app_port(&self, id: Uuid, port: i32) -> anyhow::Result<()>;

    async fn create_deployment(&self, data: NewDeployment) -> anyhow::Result<Deployment>;
    async fn update_deployment_status(
        &self,
        id: Uuid,
        status: &str,
        job_id: Option<String>,
        image_tag: Option<String>,
        build_id: Option<String>,
        ip_address: Option<String>,
    ) -> anyhow::Result<()>;
    async fn update_deployment_port(&self, id: Uuid, port: i32) -> anyhow::Result<()>;
    async fn get_deployment(&self, id: Uuid) -> anyhow::Result<Option<Deployment>>;
    async fn get_deployment_by_job_id(&self, job_id: &str) -> anyhow::Result<Option<Deployment>>;
    async fn list_deployments_by_app(&self, app_id: Uuid) -> anyhow::Result<Vec<Deployment>>;
    async fn list_deployments_by_user(&self, user_id: &str) -> anyhow::Result<Vec<Deployment>>;
    async fn delete_deployment_by_job_id(&self, job_id: &str) -> anyhow::Result<()>;
}
