use crate::AppState;
use crate::application::tenant::{
    require_tenant_admin_by_slug, resolve_tenant_for_user_by_slug, tenant_has_dependent_resources,
};
use crate::domain::{DomainError, Tenant};
use crate::error::{ApiError, ApiResult};
use crate::infrastructure::auth::extractor::AuthUser;
use axum::{Json, extract::Path, extract::State, http::StatusCode};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct CreateProjectRequest {
    pub name: String,
}

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct UpdateProjectRequest {
    pub name: String,
}

fn normalize_project_name(name: &str) -> ApiResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("Project name is required".to_string()));
    }

    Ok(trimmed.to_string())
}

#[rovo::rovo]
pub async fn list_projects(
    auth: AuthUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<Tenant>>> {
    let user_id = Uuid::parse_str(&auth.user_id)
        .map_err(|_| ApiError::Auth("Invalid user ID in token".into()))?;
    let tenants = state.tenant_repo.list_by_user(user_id).await?;
    Ok(Json(tenants))
}

#[rovo::rovo]
pub async fn get_project(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(tenant_slug): Path<String>,
) -> ApiResult<Json<Tenant>> {
    let tenant = resolve_tenant_for_user_by_slug(&state, &auth, &tenant_slug).await?;
    Ok(Json(tenant))
}

#[rovo::rovo]
pub async fn create_project(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<CreateProjectRequest>,
) -> ApiResult<(StatusCode, Json<Tenant>)> {
    let user_id = Uuid::parse_str(&auth.user_id)
        .map_err(|_| ApiError::Auth("Invalid user ID in token".into()))?;
    let name = normalize_project_name(&payload.name)?;

    let tenant = {
        let mut created = None;
        for _ in 0..5 {
            let slug = Tenant::generate_slug();
            match state.tenant_repo.create(name.clone(), slug).await {
                Ok(tenant) => {
                    created = Some(tenant);
                    break;
                },
                Err(DomainError::Conflict(_)) => continue,
                Err(error) => return Err(error.into()),
            }
        }

        created.ok_or_else(|| {
            ApiError::Conflict("Unable to generate a unique tenant slug".to_string())
        })?
    };

    state
        .tenant_repo
        .add_member(tenant.id, user_id, "admin")
        .await?;

    let default_tier = state.ctx.plan_tier_repo.get_default_tier().await?;
    state
        .ctx
        .plan_tier_repo
        .assign_to_tenant(tenant.id, &default_tier.tier_slug)
        .await?;

    state.ctx.tenant_usage_repo.get_or_create(tenant.id).await?;

    Ok((StatusCode::CREATED, Json(tenant)))
}

#[rovo::rovo]
pub async fn update_project(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(tenant_slug): Path<String>,
    Json(payload): Json<UpdateProjectRequest>,
) -> ApiResult<Json<Tenant>> {
    let tenant = require_tenant_admin_by_slug(&state, &auth, &tenant_slug).await?;
    let name = normalize_project_name(&payload.name)?;

    let updated = state.tenant_repo.update(tenant.id, name).await?;
    Ok(Json(updated))
}

#[rovo::rovo]
pub async fn delete_project(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(tenant_slug): Path<String>,
) -> ApiResult<StatusCode> {
    let tenant = require_tenant_admin_by_slug(&state, &auth, &tenant_slug).await?;

    if tenant_has_dependent_resources(&state, tenant.id).await? {
        return Err(ApiError::Conflict(
            "This project still has apps, databases or volumes. Remove them first.".to_string(),
        ));
    }

    let deleted = state.tenant_repo.delete(tenant.id).await?;
    if !deleted {
        return Err(ApiError::NotFound("Tenant not found".to_string()));
    }

    Ok(StatusCode::NO_CONTENT)
}
