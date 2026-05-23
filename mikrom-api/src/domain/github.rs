use crate::domain::error::DomainResult;
use async_trait::async_trait;
use uuid::Uuid;

pub use crate::models::github::UserGithubAccount;

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
