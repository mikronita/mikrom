use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::error::DomainResult;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TierSlug {
    Free,
    Hobby,
    Pro,
    Enterprise,
}

impl TierSlug {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Hobby => "hobby",
            Self::Pro => "pro",
            Self::Enterprise => "enterprise",
        }
    }

    pub fn from_slug(s: &str) -> Option<Self> {
        match s {
            "free" => Some(Self::Free),
            "hobby" => Some(Self::Hobby),
            "pro" => Some(Self::Pro),
            "enterprise" => Some(Self::Enterprise),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanTier {
    pub id: Uuid,
    pub polar_product_id: Option<String>,
    pub tier_slug: TierSlug,
    pub name: String,
    pub max_apps: i32,
    pub max_databases: i32,
    pub max_volumes: i32,
    pub max_vcpus_total: i32,
    pub max_memory_mb_total: i32,
    pub max_storage_gb_total: i32,
    pub max_deployments_per_app: i32,
    pub max_team_members: i32,
    pub autoscaling_allowed: bool,
    pub custom_domains: bool,
    pub trial_days: i32,
    pub is_default: bool,
    pub sort_order: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, rovo::schemars::JsonSchema)]
pub struct PlanLimits {
    pub max_apps: i32,
    pub max_databases: i32,
    pub max_volumes: i32,
    pub max_vcpus_total: i32,
    pub max_memory_mb_total: i32,
    pub max_storage_gb_total: i32,
    pub max_deployments_per_app: i32,
    pub max_team_members: i32,
    pub autoscaling_allowed: bool,
    pub custom_domains: bool,
}

impl From<&PlanTier> for PlanLimits {
    fn from(tier: &PlanTier) -> Self {
        Self {
            max_apps: tier.max_apps,
            max_databases: tier.max_databases,
            max_volumes: tier.max_volumes,
            max_vcpus_total: tier.max_vcpus_total,
            max_memory_mb_total: tier.max_memory_mb_total,
            max_storage_gb_total: tier.max_storage_gb_total,
            max_deployments_per_app: tier.max_deployments_per_app,
            max_team_members: tier.max_team_members,
            autoscaling_allowed: tier.autoscaling_allowed,
            custom_domains: tier.custom_domains,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantUsage {
    pub tenant_id: Uuid,
    pub apps_count: i32,
    pub databases_count: i32,
    pub volumes_count: i32,
    pub vcpus_total: i32,
    pub memory_mb_total: i32,
    pub storage_gb_total: i32,
    pub deployments_count: i32,
    pub bandwidth_gb_billed: i32,
    pub updated_at: DateTime<Utc>,
}

#[mockall::automock]
#[async_trait]
pub trait PlanTierRepository: Send + Sync {
    async fn get_default_tier(&self) -> DomainResult<PlanTier>;
    async fn get_by_slug(&self, slug: &TierSlug) -> DomainResult<Option<PlanTier>>;
    async fn get_by_polar_product_id(
        &self,
        polar_product_id: &str,
    ) -> DomainResult<Option<PlanTier>>;
    async fn list_all(&self) -> DomainResult<Vec<PlanTier>>;
    async fn assign_to_tenant(&self, tenant_id: Uuid, tier_slug: &TierSlug) -> DomainResult<()>;
    async fn get_tenant_tier(&self, tenant_id: Uuid) -> DomainResult<PlanTier>;
}

#[mockall::automock]
#[async_trait]
pub trait TenantUsageRepository: Send + Sync {
    async fn get_or_create(&self, tenant_id: Uuid) -> DomainResult<TenantUsage>;
    async fn increment_apps(
        &self,
        tenant_id: Uuid,
        delta: i32,
        vcpus: i32,
        memory_mb: i32,
        storage_gb: i32,
    ) -> DomainResult<()>;
    async fn decrement_apps(
        &self,
        tenant_id: Uuid,
        vcpus: i32,
        memory_mb: i32,
        storage_gb: i32,
    ) -> DomainResult<()>;
    async fn increment_databases(&self, tenant_id: Uuid, delta: i32) -> DomainResult<()>;
    async fn decrement_databases(&self, tenant_id: Uuid) -> DomainResult<()>;
    async fn increment_volumes(
        &self,
        tenant_id: Uuid,
        delta: i32,
        storage_gb: i32,
    ) -> DomainResult<()>;
    async fn decrement_volumes(&self, tenant_id: Uuid, storage_gb: i32) -> DomainResult<()>;
    async fn increment_deployments(&self, tenant_id: Uuid, delta: i32) -> DomainResult<()>;
    async fn decrement_deployments(&self, tenant_id: Uuid) -> DomainResult<()>;
}
