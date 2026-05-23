// Thin compatibility layer re-exporting domain definitions.
pub use crate::domain::github::{GithubRepository, UserGithubAccount};

#[derive(Default)]
pub struct MockGithubRepository {}

#[async_trait::async_trait]
impl GithubRepository for MockGithubRepository {
    async fn create_account(
        &self,
        account: UserGithubAccount,
    ) -> crate::domain::DomainResult<UserGithubAccount> {
        Ok(account)
    }
    async fn get_account_by_installation_id(
        &self,
        _installation_id: i64,
    ) -> crate::domain::DomainResult<Option<UserGithubAccount>> {
        Ok(None)
    }
    async fn get_accounts_by_user_id(
        &self,
        _user_id: uuid::Uuid,
    ) -> crate::domain::DomainResult<Vec<UserGithubAccount>> {
        Ok(vec![])
    }
    async fn delete_account(&self, _id: uuid::Uuid) -> crate::domain::DomainResult<()> {
        Ok(())
    }
}
