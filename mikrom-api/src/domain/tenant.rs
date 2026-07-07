use crate::domain::error::DomainResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, rovo::schemars::JsonSchema)]
pub struct Tenant {
    pub id: Uuid,
    pub tenant_id: String, // The 6-char slug
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Tenant {
    pub fn generate_slug() -> String {
        use rand::distr::{Alphanumeric, SampleString};
        use rand::rng;

        Alphanumeric.sample_string(&mut rng(), 6).to_lowercase()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, rovo::schemars::JsonSchema)]
pub struct TenantMember {
    pub tenant_id: Uuid,
    pub user_id: Uuid,
    pub role: String,
}

#[mockall::automock]
#[async_trait]
pub trait TenantRepository: Send + Sync {
    async fn create(&self, name: String, slug: String) -> DomainResult<Tenant>;
    async fn find_by_id(&self, id: Uuid) -> DomainResult<Option<Tenant>>;
    async fn find_by_slug(&self, slug: &str) -> DomainResult<Option<Tenant>>;
    async fn list_by_user(&self, user_id: Uuid) -> DomainResult<Vec<Tenant>>;
    async fn update(&self, tenant_id: Uuid, name: String) -> DomainResult<Tenant>;
    async fn list_all(&self) -> DomainResult<Vec<Tenant>>;
    async fn delete(&self, tenant_id: Uuid) -> DomainResult<bool>;
    async fn add_member(&self, tenant_id: Uuid, user_id: Uuid, role: &str) -> DomainResult<()>;
    async fn get_members(&self, tenant_id: Uuid) -> DomainResult<Vec<TenantMember>>;
    async fn is_member(&self, tenant_id: Uuid, user_id: Uuid) -> DomainResult<bool>;
}
