use crate::domain::error::DomainResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(
    Default, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub enum UserRole {
    Admin,
    #[default]
    User,
}

#[derive(Debug, Clone)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub role: UserRole,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub avatar_url: Option<String>,
    pub vpc_ipv6_prefix: Option<String>,
    pub totp_secret: Option<String>,
    pub totp_enabled: bool,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewUser {
    pub email: String,
    pub password_hash: String,
    pub role: UserRole,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub avatar_url: Option<String>,
}

#[mockall::automock]
#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn find_by_email(&self, email: &str) -> DomainResult<Option<User>>;
    async fn find_by_id(&self, id: Uuid) -> DomainResult<Option<User>>;
    async fn create(&self, user: NewUser) -> DomainResult<Uuid>;
    async fn count_by_email(&self, email: &str) -> DomainResult<i64>;
    async fn update_profile(
        &self,
        id: Uuid,
        first_name: Option<String>,
        last_name: Option<String>,
        avatar_url: Option<String>,
    ) -> DomainResult<User>;
    async fn update_password(&self, id: Uuid, new_password_hash: String) -> DomainResult<()>;
    async fn update_totp_secret(&self, id: Uuid, secret: Option<String>) -> DomainResult<()>;
    async fn enable_totp(&self, id: Uuid) -> DomainResult<()>;
    async fn disable_totp(&self, id: Uuid) -> DomainResult<()>;
    async fn soft_delete(&self, id: Uuid) -> DomainResult<()>;
}
