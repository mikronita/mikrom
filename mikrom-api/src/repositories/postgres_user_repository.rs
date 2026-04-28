use async_trait::async_trait;
use sqlx::PgPool;

use super::user_repository::{DbError, NewUser, User, UserRepository, UserRole};

pub struct PostgresUserRepository {
    pool: PgPool,
}

impl PostgresUserRepository {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRepository for PostgresUserRepository {
    async fn find_by_email(&self, email: &str) -> Result<Option<User>, DbError> {
        let result = sqlx::query_as::<_, (sqlx::types::Uuid, String, String, String, Option<String>, Option<String>)>(
            "SELECT id, email, password_hash, role, first_name, last_name FROM users WHERE email = $1",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((id, email, password_hash, role_str, first_name, last_name)) = result {
            let role = match role_str.as_str() {
                "admin" => UserRole::Admin,
                _ => UserRole::User,
            };
            Ok(Some(User {
                id,
                email,
                password_hash,
                role,
                first_name,
                last_name,
            }))
        } else {
            Ok(None)
        }
    }

    async fn find_by_id(&self, id: sqlx::types::Uuid) -> Result<Option<User>, DbError> {
        let result = sqlx::query_as::<
            _,
            (
                sqlx::types::Uuid,
                String,
                String,
                String,
                Option<String>,
                Option<String>,
            ),
        >(
            "SELECT id, email, password_hash, role, first_name, last_name FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((id, email, password_hash, role_str, first_name, last_name)) = result {
            let role = match role_str.as_str() {
                "admin" => UserRole::Admin,
                _ => UserRole::User,
            };
            Ok(Some(User {
                id,
                email,
                password_hash,
                role,
                first_name,
                last_name,
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
        let result = sqlx::query("INSERT INTO users (id, email, password_hash, role, first_name, last_name) VALUES ($1, $2, $3, $4, $5, $6)")
            .bind(id)
            .bind(&user.email)
            .bind(&user.password_hash)
            .bind(role_str)
            .bind(&user.first_name)
            .bind(&user.last_name)
            .execute(&self.pool)
            .await;

        match result {
            Ok(_) => Ok(id),
            Err(e) => {
                if let Some(db_err) = e.as_database_error()
                    && db_err.code().as_deref() == Some("23505")
                {
                    return Err(DbError::Conflict(format!(
                        "Email '{}' is already registered",
                        user.email
                    )));
                }
                Err(DbError::from(e))
            },
        }
    }

    async fn count_by_email(&self, email: &str) -> Result<i64, DbError> {
        let (count,) = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM users WHERE email = $1")
            .bind(email)
            .fetch_one(&self.pool)
            .await?;

        Ok(count)
    }

    async fn update_profile(
        &self,
        id: sqlx::types::Uuid,
        first_name: Option<String>,
        last_name: Option<String>,
    ) -> Result<User, DbError> {
        let result = sqlx::query_as::<
            _,
            (
                sqlx::types::Uuid,
                String,
                String,
                String,
                Option<String>,
                Option<String>,
            ),
        >(
            "UPDATE users SET first_name = COALESCE($1, first_name), last_name = COALESCE($2, last_name) \
             WHERE id = $3 RETURNING id, email, password_hash, role, first_name, last_name",
        )
        .bind(first_name)
        .bind(last_name)
        .bind(id)
        .fetch_one(&self.pool)
        .await?;

        let (id, email, password_hash, role_str, first_name, last_name) = result;
        let role = match role_str.as_str() {
            "admin" => UserRole::Admin,
            _ => UserRole::User,
        };

        Ok(User {
            id,
            email,
            password_hash,
            role,
            first_name,
            last_name,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lazy_pool() -> PgPool {
        let url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
            "postgres://mikrom:mikrom_password@localhost:5432/mikrom_api_test".to_string()
        });
        PgPool::connect_lazy(&url).expect("invalid pool URL")
    }

    #[tokio::test]
    async fn test_new_creates_instance_without_panicking() {
        let _repo = PostgresUserRepository::new(lazy_pool());
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL"]
    async fn test_find_by_email_returns_none_for_unknown_email() {
        let pool = PgPool::connect(&std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
            "postgres://mikrom:mikrom_password@localhost:5432/mikrom_api_test".to_string()
        }))
        .await
        .expect("failed to connect");
        let repo = PostgresUserRepository::new(pool);
        let result: Result<Option<User>, DbError> =
            repo.find_by_email("nonexistent@example.com").await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL"]
    async fn test_create_and_find_roundtrip() {
        let pool = PgPool::connect(&std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
            "postgres://mikrom:mikrom_password@localhost:5432/mikrom_api_test".to_string()
        }))
        .await
        .expect("failed to connect");
        let repo = PostgresUserRepository::new(pool);
        let email = format!("repo_test_{}@example.com", uuid::Uuid::new_v4());
        let id = repo
            .create(NewUser {
                email: email.clone(),
                password_hash: "x".to_string(),
                role: UserRole::User,
                first_name: None,
                last_name: None,
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
            "postgres://mikrom:mikrom_password@localhost:5432/mikrom_api_test".to_string()
        }))
        .await
        .expect("failed to connect");
        let repo = PostgresUserRepository::new(pool);
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
            "postgres://mikrom:mikrom_password@localhost:5432/mikrom_api_test".to_string()
        }))
        .await
        .expect("failed to connect");
        let repo = PostgresUserRepository::new(pool);
        let email = format!("count_test_{}@example.com", uuid::Uuid::new_v4());
        repo.create(NewUser {
            email: email.clone(),
            password_hash: "x".to_string(),
            role: UserRole::User,
            first_name: None,
            last_name: None,
        })
        .await
        .expect("create failed");
        let count: i64 = repo.count_by_email(&email).await.expect("count failed");
        assert_eq!(count, 1);
    }
}
