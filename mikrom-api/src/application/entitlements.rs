use uuid::Uuid;

use std::sync::Arc;

use crate::AppState;
use crate::domain::plan_tier::{
    PlanLimits, PlanTierRepository, TenantUsage, TenantUsageRepository,
};
use crate::domain::{DomainError, DomainResult};
use crate::error::ApiResult;

pub enum EntitlementCheck {
    CreateApp {
        vcpus: i32,
        memory_mb: i32,
        storage_gb: i32,
        autoscaling: bool,
    },
    CreateDeployment,
    CreateDatabase,
    CreateVolume {
        size_gb: i32,
    },
    EnableAutoscaling,
    CustomDomains,
    AddTeamMember,
}

#[derive(Debug, Clone)]
pub enum EntitlementError {
    AppLimitReached { current: i32, max: i32 },
    DatabaseLimitReached { current: i32, max: i32 },
    VolumeLimitReached { current: i32, max: i32 },
    VcpuLimitReached { current: i32, max: i32 },
    MemoryLimitReached { current: i32, max: i32 },
    StorageLimitReached { current: i32, max: i32 },
    DeploymentLimitReached { current: i32, max: i32 },
    TeamMemberLimitReached { current: i32, max: i32 },
    AutoscalingNotAllowed,
    CustomDomainsNotAllowed,
}

/// Ensures all existing tenants have a plan tier assigned.
/// Called once on service startup.
pub async fn migrate_existing_tenants(state: &AppState) -> ApiResult<()> {
    let tenants = state
        .ctx
        .tenant_repo
        .list_all()
        .await
        .map_err(|e| crate::error::ApiError::Internal(e.to_string()))?;

    let count = tenants.len();
    let ent_service = EntitlementService {
        plan_tier_repo: state.ctx.plan_tier_repo.clone(),
        usage_repo: state.ctx.tenant_usage_repo.clone(),
    };

    for tenant in &tenants {
        if let Err(e) = ent_service.ensure_tenant_has_tier(tenant.id).await {
            tracing::warn!(tenant_id = %tenant.id, error = %e, "Failed to assign tier for tenant");
        }
    }

    tracing::info!(count, "Ensured plan tiers for existing tenants");
    Ok(())
}

impl std::fmt::Display for EntitlementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AppLimitReached { current, max } => {
                write!(f, "App limit reached ({current}/{max})")
            },
            Self::DatabaseLimitReached { current, max } => {
                write!(f, "Database limit reached ({current}/{max})")
            },
            Self::VolumeLimitReached { current, max } => {
                write!(f, "Volume limit reached ({current}/{max})")
            },
            Self::VcpuLimitReached { current, max } => {
                write!(f, "vCPU limit reached ({current}/{max})")
            },
            Self::MemoryLimitReached { current, max } => {
                write!(f, "Memory limit reached ({current}/{max})")
            },
            Self::StorageLimitReached { current, max } => {
                write!(f, "Storage limit reached ({current}/{max})")
            },
            Self::DeploymentLimitReached { current, max } => {
                write!(f, "Deployment limit reached ({current}/{max})")
            },
            Self::TeamMemberLimitReached { current, max } => {
                write!(f, "Team member limit reached ({current}/{max})")
            },
            Self::AutoscalingNotAllowed => write!(f, "Autoscaling is not allowed on your plan"),
            Self::CustomDomainsNotAllowed => {
                write!(f, "Custom domains are not allowed on your plan")
            },
        }
    }
}

impl From<EntitlementError> for DomainError {
    fn from(e: EntitlementError) -> Self {
        DomainError::InvalidRequest(e.to_string())
    }
}

pub struct EntitlementService {
    plan_tier_repo: Arc<dyn PlanTierRepository>,
    usage_repo: Arc<dyn TenantUsageRepository>,
}

impl EntitlementService {
    pub fn new(
        plan_tier_repo: Arc<dyn PlanTierRepository>,
        usage_repo: Arc<dyn TenantUsageRepository>,
    ) -> Self {
        Self {
            plan_tier_repo,
            usage_repo,
        }
    }

    pub async fn check_entitlement(
        &self,
        tenant_id: Uuid,
        check: EntitlementCheck,
    ) -> DomainResult<()> {
        let tier = self.plan_tier_repo.get_tenant_tier(tenant_id).await?;
        let limits = PlanLimits::from(&tier);
        let usage = self.usage_repo.get_or_create(tenant_id).await?;

        match check {
            EntitlementCheck::CreateApp {
                vcpus,
                memory_mb,
                storage_gb,
                autoscaling: _autoscaling,
            } => {
                Self::check_limit(usage.apps_count + 1, limits.max_apps, |c, m| {
                    EntitlementError::AppLimitReached { current: c, max: m }
                })?;
                Self::check_limit(usage.vcpus_total + vcpus, limits.max_vcpus_total, |c, m| {
                    EntitlementError::VcpuLimitReached { current: c, max: m }
                })?;
                Self::check_limit(
                    usage.memory_mb_total + memory_mb,
                    limits.max_memory_mb_total,
                    |c, m| EntitlementError::MemoryLimitReached { current: c, max: m },
                )?;
                let new_storage_gb = usage.storage_gb_total + storage_gb;
                if new_storage_gb > limits.max_storage_gb_total {
                    return Err(EntitlementError::StorageLimitReached {
                        current: new_storage_gb,
                        max: limits.max_storage_gb_total,
                    }
                    .into());
                }
            },
            EntitlementCheck::CreateDeployment => {
                Self::check_limit(
                    usage.deployments_count + 1,
                    limits.max_deployments_per_app,
                    |c, m| EntitlementError::DeploymentLimitReached { current: c, max: m },
                )?;
            },
            EntitlementCheck::CreateDatabase => {
                Self::check_limit(usage.databases_count + 1, limits.max_databases, |c, m| {
                    EntitlementError::DatabaseLimitReached { current: c, max: m }
                })?;
            },
            EntitlementCheck::CreateVolume { size_gb } => {
                Self::check_limit(usage.volumes_count + 1, limits.max_volumes, |c, m| {
                    EntitlementError::VolumeLimitReached { current: c, max: m }
                })?;
                let new_storage_gb = usage.storage_gb_total + size_gb;
                if new_storage_gb > limits.max_storage_gb_total {
                    return Err(EntitlementError::StorageLimitReached {
                        current: new_storage_gb,
                        max: limits.max_storage_gb_total,
                    }
                    .into());
                }
            },
            EntitlementCheck::EnableAutoscaling => {
                if !limits.autoscaling_allowed {
                    return Err(EntitlementError::AutoscalingNotAllowed.into());
                }
            },
            EntitlementCheck::CustomDomains => {
                if !limits.custom_domains {
                    return Err(EntitlementError::CustomDomainsNotAllowed.into());
                }
            },
            EntitlementCheck::AddTeamMember => {
                let members = self.plan_tier_repo.list_all().await?;
                let current_members = members.len() as i32;
                Self::check_limit(current_members + 1, limits.max_team_members, |c, m| {
                    EntitlementError::TeamMemberLimitReached { current: c, max: m }
                })?;
            },
        }

        Ok(())
    }

    pub async fn get_tenant_limits(
        &self,
        tenant_id: Uuid,
    ) -> DomainResult<(PlanLimits, TenantUsage)> {
        let tier = self.plan_tier_repo.get_tenant_tier(tenant_id).await?;
        let limits = PlanLimits::from(&tier);
        let usage = self.usage_repo.get_or_create(tenant_id).await?;
        Ok((limits, usage))
    }

    pub async fn ensure_tenant_has_tier(&self, tenant_id: Uuid) -> DomainResult<()> {
        let default_tier = self.plan_tier_repo.get_default_tier().await?;
        self.plan_tier_repo
            .assign_to_tenant(tenant_id, &default_tier.tier_slug)
            .await?;
        self.usage_repo.get_or_create(tenant_id).await?;
        Ok(())
    }

    fn check_limit<F: Fn(i32, i32) -> EntitlementError>(
        current: i32,
        max: i32,
        error_fn: F,
    ) -> DomainResult<()> {
        if current > max {
            return Err(error_fn(current, max).into());
        }
        Ok(())
    }
}
