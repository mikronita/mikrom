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
    pub active_deployment_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone, ToSchema)]
pub struct Deployment {
    pub id: Uuid,
    pub app_id: Uuid,
    pub user_id: Uuid,
    pub build_id: Option<String>,
    pub image_tag: Option<String>,
    pub job_id: Option<String>,
    pub ip_address: Option<String>,
    pub status: String,
    pub vcpus: i32,
    pub memory_mib: i64,
    pub disk_mib: i64,
    pub port: i32,
    #[schema(value_type = Object)]
    pub env_vars: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
