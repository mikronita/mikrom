use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub id: String,
    pub user_id: String,
    pub vpc_ipv6_prefix: String,
    pub hostname: String,
    pub desired_replicas: u32,
    pub min_replicas: u32,
    pub max_replicas: u32,
    pub autoscaling_enabled: bool,
    pub cpu_threshold: f64,
    pub mem_threshold: f64,
    pub last_router_traffic_at: i64,
    pub last_scaled_to_zero_at: i64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            user_id: String::new(),
            vpc_ipv6_prefix: String::new(),
            hostname: String::new(),
            desired_replicas: 0,
            min_replicas: 0,
            max_replicas: 0,
            autoscaling_enabled: false,
            cpu_threshold: 0.0,
            mem_threshold: 0.0,
            last_router_traffic_at: 0,
            last_scaled_to_zero_at: 0,
        }
    }
}

#[async_trait::async_trait]
#[mockall::automock]
pub trait AppRepository: Send + Sync {
    async fn update_app_config(&self, config: AppConfig) -> anyhow::Result<()>;
    async fn get_app_config(&self, app_id: &str) -> anyhow::Result<Option<AppConfig>>;
    async fn get_app_config_by_hostname(&self, hostname: &str)
    -> anyhow::Result<Option<AppConfig>>;
    async fn list_all_apps(&self) -> anyhow::Result<Vec<AppConfig>>;
    async fn list_autoscaling_apps(&self) -> anyhow::Result<Vec<AppConfig>>;
    async fn remove_app_config(&self, app_id: &str) -> anyhow::Result<()>;
}
