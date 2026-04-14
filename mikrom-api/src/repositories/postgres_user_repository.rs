use sqlx::PgPool;
use std::sync::Arc;

use super::user_repository::{DbError, NewUser, User, UserRepository};

pub struct PostgresUserRepository {
    pool: Arc<PgPool>,
}

impl PostgresUserRepository {
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }
}

impl UserRepository for PostgresUserRepository {
    async fn find_by_email(&self, email: &str) -> Result<Option<User>, DbError> {
        let result = sqlx::query_as::<_, (sqlx::types::Uuid, String, String)>(
            "SELECT id, email, password_hash FROM users WHERE email = $1"
        )
        .bind(email)
        .fetch_optional(&*self.pool)
        .await;

        match result {
            Ok(Some((id, email, password_hash))) => Ok(Some(User { id, email, password_hash })),
            Ok(None) => Ok(None),
            Err(e) => Err(DbError::from(e)),
        }
    }

    async fn create(&self, user: NewUser) -> Result<sqlx::types::Uuid, DbError> {
        let id = sqlx::types::Uuid::new_v4();
        let result = sqlx::query(
            "INSERT INTO users (id, email, password_hash) VALUES ($1, $2, $3)"
        )
        .bind(id)
        .bind(&user.email)
        .bind(&user.password_hash)
        .execute(&*self.pool)
        .await;

        match result {
            Ok(_) => Ok(id),
            Err(e) => Err(DbError::from(e)),
        }
    }

    async fn count_by_email(&self, email: &str) -> Result<i64, DbError> {
        let result = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM users WHERE email = $1"
        )
        .bind(email)
        .fetch_one(&*self.pool)
        .await;

        match result {
            Ok((count,)) => Ok(count),
            Err(e) => Err(DbError::from(e)),
        }
    }
}
