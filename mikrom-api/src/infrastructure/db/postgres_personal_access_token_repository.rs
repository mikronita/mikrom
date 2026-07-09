use crate::domain::User;
use crate::domain::UserRole;
use crate::domain::error::DomainResult;
use crate::domain::personal_access_token::{PersonalAccessToken, PersonalAccessTokenRepository};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

pub struct PostgresPersonalAccessTokenRepository {
    pool: PgPool,
}

impl PostgresPersonalAccessTokenRepository {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(Debug, FromRow)]
struct DbPersonalAccessToken {
    id: Uuid,
    user_id: Uuid,
    name: String,
    token_last_four: String,
    created_at: DateTime<Utc>,
    last_used_at: Option<DateTime<Utc>>,
}

impl From<DbPersonalAccessToken> for PersonalAccessToken {
    fn from(db: DbPersonalAccessToken) -> Self {
        Self {
            id: db.id,
            user_id: db.user_id,
            name: db.name,
            token_last_four: db.token_last_four,
            created_at: db.created_at,
            last_used_at: db.last_used_at,
        }
    }
}

#[derive(Debug, FromRow)]
struct JoinedPatRow {
    t_id: Uuid,
    t_user_id: Uuid,
    t_name: String,
    t_token_last_four: String,
    t_created_at: DateTime<Utc>,
    t_last_used_at: Option<DateTime<Utc>>,
    u_id: Uuid,
    u_email: String,
    u_password_hash: String,
    u_role: String,
    u_first_name: Option<String>,
    u_last_name: Option<String>,
    u_avatar_url: Option<String>,
    u_vpc_ipv6_prefix: Option<String>,
    u_totp_secret: Option<String>,
    u_totp_enabled: bool,
    u_deleted_at: Option<DateTime<Utc>>,
}

#[async_trait]
impl PersonalAccessTokenRepository for PostgresPersonalAccessTokenRepository {
    async fn create(
        &self,
        id: Uuid,
        user_id: Uuid,
        name: String,
        token_hash: String,
        token_last_four: String,
    ) -> DomainResult<PersonalAccessToken> {
        let db_created = sqlx::query_as::<_, DbPersonalAccessToken>(
            "INSERT INTO personal_access_tokens (id, user_id, name, token_hash, token_last_four)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id, user_id, name, token_last_four, created_at, last_used_at",
        )
        .bind(id)
        .bind(user_id)
        .bind(name)
        .bind(token_hash)
        .bind(token_last_four)
        .fetch_one(&self.pool)
        .await?;

        Ok(db_created.into())
    }

    async fn list_by_user(&self, user_id: Uuid) -> DomainResult<Vec<PersonalAccessToken>> {
        let db_tokens = sqlx::query_as::<_, DbPersonalAccessToken>(
            "SELECT id, user_id, name, token_last_four, created_at, last_used_at
             FROM personal_access_tokens
             WHERE user_id = $1
             ORDER BY created_at DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(db_tokens.into_iter().map(Into::into).collect())
    }

    async fn find_by_hash(
        &self,
        token_hash: &str,
    ) -> DomainResult<Option<(PersonalAccessToken, User)>> {
        let result = sqlx::query_as::<_, JoinedPatRow>(
            "SELECT 
                t.id AS t_id, t.user_id AS t_user_id, t.name AS t_name, t.token_last_four AS t_token_last_four, t.created_at AS t_created_at, t.last_used_at AS t_last_used_at,
                u.id AS u_id, u.email AS u_email, u.password_hash AS u_password_hash, u.role AS u_role, u.first_name AS u_first_name, u.last_name AS u_last_name, u.avatar_url AS u_avatar_url, u.vpc_ipv6_prefix AS u_vpc_ipv6_prefix, u.totp_secret AS u_totp_secret, u.totp_enabled AS u_totp_enabled, u.deleted_at AS u_deleted_at
             FROM personal_access_tokens t
             JOIN users u ON t.user_id = u.id
             WHERE t.token_hash = $1 AND u.deleted_at IS NULL"
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = result {
            let role = match row.u_role.as_str() {
                "admin" => UserRole::Admin,
                _ => UserRole::User,
            };
            let token = PersonalAccessToken {
                id: row.t_id,
                user_id: row.t_user_id,
                name: row.t_name,
                token_last_four: row.t_token_last_four,
                created_at: row.t_created_at,
                last_used_at: row.t_last_used_at,
            };
            let user = User {
                id: row.u_id,
                email: row.u_email,
                password_hash: row.u_password_hash,
                role,
                first_name: row.u_first_name,
                last_name: row.u_last_name,
                avatar_url: row.u_avatar_url,
                vpc_ipv6_prefix: row.u_vpc_ipv6_prefix,
                totp_secret: row.u_totp_secret,
                totp_enabled: row.u_totp_enabled,
                deleted_at: row.u_deleted_at,
            };
            Ok(Some((token, user)))
        } else {
            Ok(None)
        }
    }

    async fn delete(&self, id: Uuid, user_id: Uuid) -> DomainResult<bool> {
        let result =
            sqlx::query("DELETE FROM personal_access_tokens WHERE id = $1 AND user_id = $2")
                .bind(id)
                .bind(user_id)
                .execute(&self.pool)
                .await?;

        Ok(result.rows_affected() > 0)
    }

    async fn update_last_used(&self, id: Uuid) -> DomainResult<()> {
        sqlx::query("UPDATE personal_access_tokens SET last_used_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
