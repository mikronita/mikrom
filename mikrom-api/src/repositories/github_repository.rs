use crate::models::github::UserGithubAccount;
use async_trait::async_trait;
use uuid::Uuid;

#[async_trait]
pub trait GithubRepository: Send + Sync {
    async fn create_account(&self, account: UserGithubAccount)
    -> anyhow::Result<UserGithubAccount>;
    async fn get_account_by_installation_id(
        &self,
        installation_id: i64,
    ) -> anyhow::Result<Option<UserGithubAccount>>;
    async fn get_accounts_by_user_id(
        &self,
        user_id: Uuid,
    ) -> anyhow::Result<Vec<UserGithubAccount>>;
    async fn delete_account(&self, id: Uuid) -> anyhow::Result<()>;
}

#[cfg(any(test, feature = "test-utils"))]
#[derive(Default)]
pub struct MockGithubRepository {}

#[cfg(any(test, feature = "test-utils"))]
#[async_trait]
impl GithubRepository for MockGithubRepository {
    async fn create_account(
        &self,
        account: UserGithubAccount,
    ) -> anyhow::Result<UserGithubAccount> {
        Ok(account)
    }
    async fn get_account_by_installation_id(
        &self,
        _installation_id: i64,
    ) -> anyhow::Result<Option<UserGithubAccount>> {
        Ok(None)
    }
    async fn get_accounts_by_user_id(
        &self,
        _user_id: Uuid,
    ) -> anyhow::Result<Vec<UserGithubAccount>> {
        Ok(vec![])
    }
    async fn delete_account(&self, _id: Uuid) -> anyhow::Result<()> {
        Ok(())
    }
}
