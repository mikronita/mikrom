use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, FromRow, Clone, ToSchema)]
pub struct App {
    pub id: Uuid,
    pub name: String,
    pub git_url: String,
    pub port: i32,
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

#[derive(Debug, Serialize, Deserialize, FromRow, Clone, ToSchema, Default)]
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
    pub port: i32,
    #[schema(value_type = Object)]
    pub env_vars: serde_json::Value,
    pub git_commit_hash: Option<String>,
    pub git_commit_message: Option<String>,
    pub git_branch: Option<String>,
    pub trigger_source: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone, ToSchema)]
pub struct SecurityRule {
    pub id: Uuid,
    pub app_id: Uuid,
    pub protocol: String,
    pub port_start: i32,
    pub port_end: i32,
    pub action: String,
    pub priority: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
