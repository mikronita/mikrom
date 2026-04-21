use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::Arc;

use super::user_repository::{DbError, NewUser, User, UserRepository, UserRole};

pub struct PostgresUserRepository {
    pool: Arc<PgPool>,
}

impl PostgresUserRepository {
    #[must_use]
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRepository for PostgresUserRepository {
    async fn find_by_email(&self, email: &str) -> Result<Option<User>, DbError> {
        let result = sqlx::query_as::<_, (sqlx::types::Uuid, String, String, String)>(
            "SELECT id, email, password_hash, role FROM users WHERE email = $1",
        )
        .bind(email)
        .fetch_optional(&*self.pool)
        .await?;

        if let Some((id, email, password_hash, role_str)) = result {
            let role = match role_str.as_str() {
                "admin" => UserRole::Admin,
                _ => UserRole::User,
            };
            Ok(Some(User {
                id,
                email,
                password_hash,
                role,
            }))
        } else {
            Ok(None)
        }
    }

    async fn create(&self, user: NewUser) -> Result<sqlx::types::Uuid, DbError> {
        let id = sqlx::types::Uuid::new_v4();
        let role_str = match user.role {
            UserRole::Admin => "admin",
            UserRole::User => "user",
        };
        sqlx::query("INSERT INTO users (id, email, password_hash, role) VALUES ($1, $2, $3, $4)")
            .bind(id)
            .bind(&user.email)
            .bind(&user.password_hash)
            .bind(role_str)
            .execute(&*self.pool)
            .await?;

        Ok(id)
    }

    async fn count_by_email(&self, email: &str) -> Result<i64, DbError> {
        let (count,) = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM users WHERE email = $1")
            .bind(email)
            .fetch_one(&*self.pool)
            .await?;

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn lazy_pool() -> Arc<PgPool> {
        Arc::new(
            PgPool::connect_lazy("postgres://mikrom:mikrom_password@localhost:5432/mikrom_api")
                .expect("invalid pool URL"),
        )
    }

    #[tokio::test]
    async fn test_new_creates_instance_without_panicking() {
        let _repo = PostgresUserRepository::new(lazy_pool());
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL"]
    async fn test_find_by_email_returns_none_for_unknown_email() {
        let pool = PgPool::connect(&std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
            "postgres://mikrom:mikrom_password@localhost:5432/mikrom_api".to_string()
        }))
        .await
        .expect("failed to connect");
        let repo = PostgresUserRepository::new(Arc::new(pool));
        let result: Result<Option<User>, DbError> =
            repo.find_by_email("nonexistent@example.com").await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL"]
    async fn test_create_and_find_roundtrip() {
        let pool = PgPool::connect(&std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
            "postgres://mikrom:mikrom_password@localhost:5432/mikrom_api".to_string()
        }))
        .await
        .expect("failed to connect");
        let repo = PostgresUserRepository::new(Arc::new(pool));
        let email = format!("repo_test_{}@example.com", uuid::Uuid::new_v4());
        let id = repo
            .create(NewUser {
                email: email.clone(),
                password_hash: "x".to_string(),
                role: UserRole::User,
            })
            .await
            .expect("create failed");

        let user: User = repo
            .find_by_email(&email)
            .await
            .expect("find failed")
            .expect("user not found");

        assert_eq!(user.id, id);
        assert_eq!(user.email, email);
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL"]
    async fn test_count_by_email_returns_zero_for_unknown() {
        let pool = PgPool::connect(&std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
            "postgres://mikrom:mikrom_password@localhost:5432/mikrom_api".to_string()
        }))
        .await
        .expect("failed to connect");
        let repo = PostgresUserRepository::new(Arc::new(pool));
        let count: i64 = repo
            .count_by_email("nobody_ever@example.com")
            .await
            .expect("count failed");
        assert_eq!(count, 0);
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL"]
    async fn test_count_by_email_returns_one_after_create() {
        let pool = PgPool::connect(&std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
            "postgres://mikrom:mikrom_password@localhost:5432/mikrom_api".to_string()
        }))
        .await
        .expect("failed to connect");
        let repo = PostgresUserRepository::new(Arc::new(pool));
        let email = format!("count_test_{}@example.com", uuid::Uuid::new_v4());
        repo.create(NewUser {
            email: email.clone(),
            password_hash: "x".to_string(),
            role: UserRole::User,
        })
        .await
        .expect("create failed");
        let count: i64 = repo.count_by_email(&email).await.expect("count failed");
        assert_eq!(count, 1);
    }
}
