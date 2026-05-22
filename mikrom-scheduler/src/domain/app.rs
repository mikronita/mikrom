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
    pub restore_retry_after_at: i64,
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
            restore_retry_after_at: 0,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_config_default_includes_restore_backoff_field() {
        let config = AppConfig::default();
        assert_eq!(config.restore_retry_after_at, 0);
    }

    #[test]
    fn test_app_config_roundtrip_preserves_restore_backoff_field() {
        let config = AppConfig {
            id: "app-1".to_string(),
            user_id: "user-1".to_string(),
            vpc_ipv6_prefix: "fd00::".to_string(),
            hostname: "app.example.com".to_string(),
            desired_replicas: 2,
            min_replicas: 1,
            max_replicas: 3,
            autoscaling_enabled: true,
            cpu_threshold: 80.0,
            mem_threshold: 70.0,
            last_router_traffic_at: 123,
            last_scaled_to_zero_at: 456,
            restore_retry_after_at: 789,
        };

        let json = serde_json::to_string(&config).unwrap();
        let restored: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.restore_retry_after_at, 789);
        assert_eq!(restored.last_scaled_to_zero_at, 456);
    }
}
