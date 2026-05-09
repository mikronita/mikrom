use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, FromRow, Clone, ToSchema)]
pub struct Worker {
    pub id: Uuid,
    pub hostname: String,
    pub ip_address: String,
    pub wireguard_pubkey: String,
    pub last_seen_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}
