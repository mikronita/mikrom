use crate::AppState;
use crate::domain::Tenant;
use crate::error::{ApiError, ApiResult};
use crate::infrastructure::auth::extractor::AuthUser;
use uuid::Uuid;

pub(crate) async fn resolve_tenant_owner_user_id(
    state: &AppState,
    tenant_id: Uuid,
) -> ApiResult<Uuid> {
    let members = state
        .ctx
        .tenant_repo
        .get_members(tenant_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    members
        .iter()
        .find(|member| member.role == "admin")
        .or_else(|| members.first())
        .map(|member| member.user_id)
        .ok_or_else(|| ApiError::NotFound("Tenant has no members".to_string()))
}

pub(crate) async fn resolve_tenant_for_user_by_slug(
    state: &AppState,
    auth_user: &AuthUser,
    tenant_slug: &str,
) -> ApiResult<Tenant> {
    let tenant = state
        .ctx
        .tenant_repo
        .find_by_slug(tenant_slug)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or_else(|| ApiError::NotFound("Tenant not found".to_string()))?;

    let user_id = Uuid::parse_str(&auth_user.user_id)
        .map_err(|_| ApiError::Auth("Invalid user ID in token".into()))?;

    let is_member = state
        .ctx
        .tenant_repo
        .is_member(tenant.id, user_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    if !is_member {
        return Err(ApiError::Forbidden);
    }

    Ok(tenant)
}

pub(crate) async fn require_tenant_admin_by_slug(
    state: &AppState,
    auth_user: &AuthUser,
    tenant_slug: &str,
) -> ApiResult<Tenant> {
    let tenant = resolve_tenant_for_user_by_slug(state, auth_user, tenant_slug).await?;
    let user_id = Uuid::parse_str(&auth_user.user_id)
        .map_err(|_| ApiError::Auth("Invalid user ID in token".into()))?;

    let members = state
        .ctx
        .tenant_repo
        .get_members(tenant.id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let is_admin = members
        .iter()
        .any(|member| member.user_id == user_id && member.role == "admin");

    if !is_admin {
        return Err(ApiError::Forbidden);
    }

    Ok(tenant)
}

pub(crate) async fn tenant_has_dependent_resources(
    state: &AppState,
    tenant_id: Uuid,
) -> ApiResult<bool> {
    if !state
        .ctx
        .app_repo
        .list_apps_by_tenant(Some(tenant_id))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .is_empty()
    {
        return Ok(true);
    }

    if !state
        .ctx
        .database_repo
        .list_databases_by_tenant(tenant_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .is_empty()
    {
        return Ok(true);
    }

    if !state
        .ctx
        .volume_repo
        .list_volumes_by_tenant(tenant_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .is_empty()
    {
        return Ok(true);
    }

    Ok(false)
}
