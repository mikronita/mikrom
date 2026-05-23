use crate::domain::error::DomainResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone, rovo::schemars::JsonSchema, Default)]
pub struct UserGithubAccount {
    pub id: Uuid,
    pub user_id: Uuid,
    pub installation_id: i64,
    pub github_username: String,
    pub created_at: DateTime<Utc>,
}

#[mockall::automock]
#[async_trait]
pub trait GithubRepository: Send + Sync {
    async fn create_account(&self, account: UserGithubAccount) -> DomainResult<UserGithubAccount>;
    async fn get_account_by_installation_id(
        &self,
        installation_id: i64,
    ) -> DomainResult<Option<UserGithubAccount>>;
    async fn get_accounts_by_user_id(&self, user_id: Uuid) -> DomainResult<Vec<UserGithubAccount>>;
    async fn delete_account(&self, id: Uuid) -> DomainResult<()>;
}
