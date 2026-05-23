use crate::domain::error::DomainResult;
use crate::domain::user::{NewUser, User, UserRepository, UserRole};
use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

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
    async fn find_by_email(&self, email: &str) -> DomainResult<Option<User>> {
        let result = sqlx::query_as::<_, (Uuid, String, String, String, Option<String>, Option<String>, Option<String>)>(
            "SELECT id, email, password_hash, role, first_name, last_name, vpc_ipv6_prefix FROM users WHERE email = $1",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((id, email, password_hash, role_str, first_name, last_name, vpc_ipv6_prefix)) =
            result
        {
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
                vpc_ipv6_prefix,
            }))
        } else {
            Ok(None)
        }
    }

    async fn find_by_id(&self, id: Uuid) -> DomainResult<Option<User>> {
        let result = sqlx::query_as::<
            _,
            (
                Uuid,
                String,
                String,
                String,
                Option<String>,
                Option<String>,
                Option<String>,
            ),
        >(
            "SELECT id, email, password_hash, role, first_name, last_name, vpc_ipv6_prefix FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((id, email, password_hash, role_str, first_name, last_name, vpc_ipv6_prefix)) =
            result
        {
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
                vpc_ipv6_prefix,
            }))
        } else {
            Ok(None)
        }
    }

    async fn create(&self, user: NewUser) -> DomainResult<Uuid> {
        let id = Uuid::new_v4();
        let role_str = match user.role {
            UserRole::Admin => "admin",
            UserRole::User => "user",
        };

        let vpc_prefix = mikrom_proto::sixpn::SixPn::generate_vpc_prefix(id.into()).to_string();

        sqlx::query("INSERT INTO users (id, email, password_hash, role, first_name, last_name, vpc_ipv6_prefix) VALUES ($1, $2, $3, $4, $5, $6, $7)")
            .bind(id)
            .bind(&user.email)
            .bind(&user.password_hash)
            .bind(role_str)
            .bind(&user.first_name)
            .bind(&user.last_name)
            .bind(vpc_prefix)
            .execute(&self.pool)
            .await?;

        Ok(id)
    }

    async fn count_by_email(&self, email: &str) -> DomainResult<i64> {
        let (count,) = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM users WHERE email = $1")
            .bind(email)
            .fetch_one(&self.pool)
            .await?;

        Ok(count)
    }

    async fn update_profile(
        &self,
        id: Uuid,
        first_name: Option<String>,
        last_name: Option<String>,
    ) -> DomainResult<User> {
        let result = sqlx::query_as::<
            _,
            (
                Uuid,
                String,
                String,
                String,
                Option<String>,
                Option<String>,
                Option<String>,
            ),
        >(
            "UPDATE users SET first_name = COALESCE($1, first_name), last_name = COALESCE($2, last_name) \
             WHERE id = $3 RETURNING id, email, password_hash, role, first_name, last_name, vpc_ipv6_prefix",
        )
        .bind(first_name)
        .bind(last_name)
        .bind(id)
        .fetch_one(&self.pool)
        .await?;

        let (id, email, password_hash, role_str, first_name, last_name, vpc_ipv6_prefix) = result;
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
            vpc_ipv6_prefix,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestDb;

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
    async fn test_find_by_email_returns_none_for_unknown_email() {
        let db = TestDb::new().await;
        let repo = PostgresUserRepository::new(db.pool().clone());
        let result = repo.find_by_email("nonexistent@example.com").await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_create_and_find_roundtrip() {
        let db = TestDb::new().await;
        let repo = PostgresUserRepository::new(db.pool().clone());
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

        let user = repo
            .find_by_email(&email)
            .await
            .expect("find failed")
            .expect("user not found");

        assert_eq!(user.id, id);
        assert_eq!(user.email, email);
    }

    #[tokio::test]
    async fn test_count_by_email_returns_zero_for_unknown() {
        let db = TestDb::new().await;
        let repo = PostgresUserRepository::new(db.pool().clone());
        let count = repo
            .count_by_email("nobody_ever@example.com")
            .await
            .expect("count failed");
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_count_by_email_returns_one_after_create() {
        let db = TestDb::new().await;
        let repo = PostgresUserRepository::new(db.pool().clone());
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
        let count = repo.count_by_email(&email).await.expect("count failed");
        assert_eq!(count, 1);
    }
}
