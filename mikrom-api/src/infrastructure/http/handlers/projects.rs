use crate::AppState;
use crate::domain::Tenant;
use crate::error::{ApiError, ApiResult};
use crate::infrastructure::auth::extractor::AuthUser;
use axum::{Json, extract::State, http::StatusCode};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct CreateProjectRequest {
    pub name: String,
}

#[rovo::rovo]
pub async fn list_projects(
    auth: AuthUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<Tenant>>> {
    let user_id = Uuid::parse_str(&auth.user_id)
        .map_err(|_| ApiError::Auth("Invalid user ID".to_string()))?;
    let tenants = state.tenant_repo.list_by_user(user_id).await?;
    Ok(Json(tenants))
}

#[rovo::rovo]
pub async fn create_project(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<CreateProjectRequest>,
) -> ApiResult<(StatusCode, Json<Tenant>)> {
    let user_id = Uuid::parse_str(&auth.user_id)
        .map_err(|_| ApiError::Auth("Invalid user ID".to_string()))?;
    let slug = Tenant::generate_slug();

    let tenant = state.tenant_repo.create(payload.name, slug).await?;
    state
        .tenant_repo
        .add_member(tenant.id, user_id, "admin")
        .await?;

    Ok((StatusCode::CREATED, Json(tenant)))
}
