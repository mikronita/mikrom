use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub id: String,
    pub user_id: String,
    pub vpc_ipv6_prefix: String,
    pub desired_replicas: u32,
    pub min_replicas: u32,
    pub max_replicas: u32,
    pub autoscaling_enabled: bool,
    pub cpu_threshold: f64,
    pub mem_threshold: f64,
}

#[async_trait::async_trait]
#[mockall::automock]
pub trait AppRepository: Send + Sync {
    async fn update_app_config(&self, config: AppConfig) -> anyhow::Result<()>;
    async fn get_app_config(&self, app_id: &str) -> anyhow::Result<Option<AppConfig>>;
    async fn list_all_apps(&self) -> anyhow::Result<Vec<AppConfig>>;
    async fn list_autoscaling_apps(&self) -> anyhow::Result<Vec<AppConfig>>;
}
