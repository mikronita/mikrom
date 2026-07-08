use crate::domain::error::DomainResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, rovo::schemars::JsonSchema)]
pub struct PersonalAccessToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub token_last_four: String,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, rovo::schemars::JsonSchema)]
pub struct CreatedTokenResponse {
    pub token: String,
    pub details: PersonalAccessToken,
}

#[mockall::automock]
#[async_trait]
pub trait PersonalAccessTokenRepository: Send + Sync {
    async fn create(
        &self,
        id: Uuid,
        user_id: Uuid,
        name: String,
        token_hash: String,
        token_last_four: String,
    ) -> DomainResult<PersonalAccessToken>;
    async fn list_by_user(&self, user_id: Uuid) -> DomainResult<Vec<PersonalAccessToken>>;
    async fn find_by_hash(&self, token_hash: &str) -> DomainResult<Option<(PersonalAccessToken, crate::domain::User)>>;
    async fn delete(&self, id: Uuid, user_id: Uuid) -> DomainResult<bool>;
    async fn update_last_used(&self, id: Uuid) -> DomainResult<()>;
}
