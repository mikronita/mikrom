use crate::AppState;
use crate::error::{ApiError, ApiResult};
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
