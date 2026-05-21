use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, FromRow, Clone, rovo::schemars::JsonSchema)]
pub struct Volume {
    pub id: Uuid,
    pub app_id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub size_mib: i32,
    pub pool_name: String,
    pub mount_point: String,
    pub access_mode: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone, rovo::schemars::JsonSchema)]
pub struct VolumeSnapshot {
    pub id: Uuid,
    pub volume_id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
}
