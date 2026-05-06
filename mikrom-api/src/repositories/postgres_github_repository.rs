use crate::models::github::UserGithubAccount;
use crate::repositories::github_repository::GithubRepository;
use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

pub struct PostgresGithubRepository {
    pool: PgPool,
}

impl PostgresGithubRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl GithubRepository for PostgresGithubRepository {
    async fn create_account(
        &self,
        account: UserGithubAccount,
    ) -> anyhow::Result<UserGithubAccount> {
        let created = sqlx::query_as::<_, UserGithubAccount>(
            "INSERT INTO user_github_accounts (user_id, installation_id, github_username) 
             VALUES ($1, $2, $3) 
             ON CONFLICT (user_id, installation_id) 
             DO UPDATE SET github_username = EXCLUDED.github_username 
             RETURNING *",
        )
        .bind(account.user_id)
        .bind(account.installation_id)
        .bind(account.github_username)
        .fetch_one(&self.pool)
        .await?;

        Ok(created)
    }

    async fn get_account_by_installation_id(
        &self,
        installation_id: i64,
    ) -> anyhow::Result<Option<UserGithubAccount>> {
        let account = sqlx::query_as::<_, UserGithubAccount>(
            "SELECT * FROM user_github_accounts WHERE installation_id = $1",
        )
        .bind(installation_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(account)
    }

    async fn get_accounts_by_user_id(
        &self,
        user_id: Uuid,
    ) -> anyhow::Result<Vec<UserGithubAccount>> {
        let accounts = sqlx::query_as::<_, UserGithubAccount>(
            "SELECT * FROM user_github_accounts WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(accounts)
    }

    async fn delete_account(&self, id: Uuid) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM user_github_accounts WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
