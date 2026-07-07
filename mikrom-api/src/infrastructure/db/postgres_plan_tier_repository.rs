use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::error::DomainResult;
use crate::domain::plan_tier::{
    PlanTier, PlanTierRepository, TenantUsage, TenantUsageRepository, TierSlug,
};
use crate::infrastructure::db::models::{DbPlanTier, DbTenantUsage};

pub struct PostgresPlanTierRepository {
    pool: PgPool,
}

impl PostgresPlanTierRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PlanTierRepository for PostgresPlanTierRepository {
    async fn get_default_tier(&self) -> DomainResult<PlanTier> {
        let db_tier = sqlx::query_as::<_, DbPlanTier>(
            "SELECT id, polar_product_id, tier_slug, name, max_apps, max_databases, max_volumes, max_vcpus_total, max_memory_mb_total, max_storage_gb_total, max_deployments_per_app, max_team_members, autoscaling_allowed, custom_domains, trial_days, is_default, sort_order, created_at FROM plan_tiers WHERE is_default = TRUE LIMIT 1",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(db_tier.into())
    }

    async fn get_by_slug(&self, slug: &TierSlug) -> DomainResult<Option<PlanTier>> {
        let db_tier = sqlx::query_as::<_, DbPlanTier>(
            "SELECT id, polar_product_id, tier_slug, name, max_apps, max_databases, max_volumes, max_vcpus_total, max_memory_mb_total, max_storage_gb_total, max_deployments_per_app, max_team_members, autoscaling_allowed, custom_domains, trial_days, is_default, sort_order, created_at FROM plan_tiers WHERE tier_slug = $1",
        )
        .bind(slug.as_str())
        .fetch_optional(&self.pool)
        .await?;
        Ok(db_tier.map(Into::into))
    }

    async fn get_by_polar_product_id(
        &self,
        polar_product_id: &str,
    ) -> DomainResult<Option<PlanTier>> {
        let db_tier = sqlx::query_as::<_, DbPlanTier>(
            "SELECT id, polar_product_id, tier_slug, name, max_apps, max_databases, max_volumes, max_vcpus_total, max_memory_mb_total, max_storage_gb_total, max_deployments_per_app, max_team_members, autoscaling_allowed, custom_domains, trial_days, is_default, sort_order, created_at FROM plan_tiers WHERE polar_product_id = $1",
        )
        .bind(polar_product_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(db_tier.map(Into::into))
    }

    async fn list_all(&self) -> DomainResult<Vec<PlanTier>> {
        let db_tiers = sqlx::query_as::<_, DbPlanTier>(
            "SELECT id, polar_product_id, tier_slug, name, max_apps, max_databases, max_volumes, max_vcpus_total, max_memory_mb_total, max_storage_gb_total, max_deployments_per_app, max_team_members, autoscaling_allowed, custom_domains, trial_days, is_default, sort_order, created_at FROM plan_tiers ORDER BY sort_order ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(db_tiers.into_iter().map(Into::into).collect())
    }

    async fn assign_to_tenant(&self, tenant_id: Uuid, tier_slug: &TierSlug) -> DomainResult<()> {
        sqlx::query(
            "INSERT INTO tenant_billing (tenant_id, plan_name, status) VALUES ($1, $2, 'active') ON CONFLICT (tenant_id) DO UPDATE SET plan_name = EXCLUDED.plan_name, updated_at = NOW()",
        )
        .bind(tenant_id)
        .bind(tier_slug.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_tenant_tier(&self, tenant_id: Uuid) -> DomainResult<PlanTier> {
        let db_tier = sqlx::query_as::<_, DbPlanTier>(
            r#"
            SELECT pt.id, pt.polar_product_id, pt.tier_slug, pt.name, pt.max_apps, pt.max_databases, pt.max_volumes, pt.max_vcpus_total, pt.max_memory_mb_total, pt.max_storage_gb_total, pt.max_deployments_per_app, pt.max_team_members, pt.autoscaling_allowed, pt.custom_domains, pt.trial_days, pt.is_default, pt.sort_order, pt.created_at
            FROM plan_tiers pt
            JOIN tenant_billing tb ON tb.plan_name = pt.tier_slug
            WHERE tb.tenant_id = $1
            "#,
        )
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await?;

        match db_tier {
            Some(tier) => Ok(tier.into()),
            None => {
                let default_tier = sqlx::query_as::<_, DbPlanTier>(
                    "SELECT id, polar_product_id, tier_slug, name, max_apps, max_databases, max_volumes, max_vcpus_total, max_memory_mb_total, max_storage_gb_total, max_deployments_per_app, max_team_members, autoscaling_allowed, custom_domains, trial_days, is_default, sort_order, created_at FROM plan_tiers WHERE is_default = TRUE LIMIT 1",
                )
                .fetch_one(&self.pool)
                .await?;
                Ok(default_tier.into())
            },
        }
    }
}

pub struct PostgresTenantUsageRepository {
    pool: PgPool,
}

impl PostgresTenantUsageRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TenantUsageRepository for PostgresTenantUsageRepository {
    async fn get_or_create(&self, tenant_id: Uuid) -> DomainResult<TenantUsage> {
        let usage = sqlx::query_as::<_, DbTenantUsage>(
            "SELECT tenant_id, apps_count, databases_count, volumes_count, vcpus_total, memory_mb_total, storage_gb_total, deployments_count, bandwidth_gb_billed, updated_at FROM tenant_usage WHERE tenant_id = $1",
        )
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(usage) = usage {
            return Ok(usage.into());
        }

        sqlx::query(
            "INSERT INTO tenant_usage (tenant_id) VALUES ($1) ON CONFLICT (tenant_id) DO NOTHING",
        )
        .bind(tenant_id)
        .execute(&self.pool)
        .await?;

        let usage = sqlx::query_as::<_, DbTenantUsage>(
            "SELECT tenant_id, apps_count, databases_count, volumes_count, vcpus_total, memory_mb_total, storage_gb_total, deployments_count, bandwidth_gb_billed, updated_at FROM tenant_usage WHERE tenant_id = $1",
        )
        .bind(tenant_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(usage.into())
    }

    async fn increment_apps(
        &self,
        tenant_id: Uuid,
        delta: i32,
        vcpus: i32,
        memory_mb: i32,
        storage_gb: i32,
    ) -> DomainResult<()> {
        sqlx::query(
            "INSERT INTO tenant_usage (tenant_id, apps_count, vcpus_total, memory_mb_total, storage_gb_total) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (tenant_id) DO UPDATE SET apps_count = tenant_usage.apps_count + $2, vcpus_total = tenant_usage.vcpus_total + $3, memory_mb_total = tenant_usage.memory_mb_total + $4, storage_gb_total = tenant_usage.storage_gb_total + $5, updated_at = NOW()",
        )
        .bind(tenant_id)
        .bind(delta)
        .bind(vcpus)
        .bind(memory_mb)
        .bind(storage_gb)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn decrement_apps(
        &self,
        tenant_id: Uuid,
        vcpus: i32,
        memory_mb: i32,
        storage_gb: i32,
    ) -> DomainResult<()> {
        sqlx::query(
            "UPDATE tenant_usage SET apps_count = GREATEST(0, apps_count - 1), vcpus_total = GREATEST(0, vcpus_total - $2), memory_mb_total = GREATEST(0, memory_mb_total - $3), storage_gb_total = GREATEST(0, storage_gb_total - $4), updated_at = NOW() WHERE tenant_id = $1",
        )
        .bind(tenant_id)
        .bind(vcpus)
        .bind(memory_mb)
        .bind(storage_gb)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn increment_databases(&self, tenant_id: Uuid, delta: i32) -> DomainResult<()> {
        sqlx::query(
            "INSERT INTO tenant_usage (tenant_id, databases_count) VALUES ($1, $2) ON CONFLICT (tenant_id) DO UPDATE SET databases_count = tenant_usage.databases_count + $2, updated_at = NOW()",
        )
        .bind(tenant_id)
        .bind(delta)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn decrement_databases(&self, tenant_id: Uuid) -> DomainResult<()> {
        sqlx::query(
            "UPDATE tenant_usage SET databases_count = GREATEST(0, databases_count - 1), updated_at = NOW() WHERE tenant_id = $1",
        )
        .bind(tenant_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn increment_volumes(
        &self,
        tenant_id: Uuid,
        delta: i32,
        storage_gb: i32,
    ) -> DomainResult<()> {
        sqlx::query(
            "INSERT INTO tenant_usage (tenant_id, volumes_count, storage_gb_total) VALUES ($1, $2, $3) ON CONFLICT (tenant_id) DO UPDATE SET volumes_count = tenant_usage.volumes_count + $2, storage_gb_total = tenant_usage.storage_gb_total + $3, updated_at = NOW()",
        )
        .bind(tenant_id)
        .bind(delta)
        .bind(storage_gb)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn decrement_volumes(&self, tenant_id: Uuid, storage_gb: i32) -> DomainResult<()> {
        sqlx::query(
            "UPDATE tenant_usage SET volumes_count = GREATEST(0, volumes_count - 1), storage_gb_total = GREATEST(0, storage_gb_total - $2), updated_at = NOW() WHERE tenant_id = $1",
        )
        .bind(tenant_id)
        .bind(storage_gb)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn increment_deployments(&self, tenant_id: Uuid, delta: i32) -> DomainResult<()> {
        sqlx::query(
            "INSERT INTO tenant_usage (tenant_id, deployments_count) VALUES ($1, $2) ON CONFLICT (tenant_id) DO UPDATE SET deployments_count = tenant_usage.deployments_count + $2, updated_at = NOW()",
        )
        .bind(tenant_id)
        .bind(delta)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn decrement_deployments(&self, tenant_id: Uuid) -> DomainResult<()> {
        sqlx::query(
            "UPDATE tenant_usage SET deployments_count = GREATEST(0, deployments_count - 1), updated_at = NOW() WHERE tenant_id = $1",
        )
        .bind(tenant_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
