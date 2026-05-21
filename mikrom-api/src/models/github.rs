use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, FromRow, Clone, rovo::schemars::JsonSchema, Default)]
pub struct UserGithubAccount {
    pub id: Uuid,
    pub user_id: Uuid,
    pub installation_id: i64,
    pub github_username: String,
    pub created_at: DateTime<Utc>,
}
