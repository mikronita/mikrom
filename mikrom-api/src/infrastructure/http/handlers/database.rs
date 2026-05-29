use crate::AppState;
use crate::application::database::DatabaseService;
use crate::auth::AuthUser;
use crate::domain::CreateDatabaseParams;
use crate::domain::types::{CpuCores, MemoryMb};
use crate::error::ApiResult;
use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct CreateDatabaseRequest {
    pub name: String,
    pub engine: String,
    pub vcpus: Option<CpuCores>,
    pub memory_mib: Option<MemoryMb>,
    pub disk_mib: Option<u32>,
    pub settings: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Serialize, rovo::schemars::JsonSchema)]
pub struct DatabaseResponse {
    pub id: Uuid,
    pub name: String,
    pub engine: String,
    pub status: String,
    pub vcpus: u32,
    pub memory_mib: u32,
    pub disk_mib: u32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[rovo::rovo]
pub async fn create_database(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<CreateDatabaseRequest>,
) -> ApiResult<Json<DatabaseResponse>> {
    let user_id = Uuid::parse_str(&auth.user_id)
        .map_err(|_| crate::error::ApiError::Auth("Invalid user ID".to_string()))?;

    let params = CreateDatabaseParams {
        name: payload.name,
        engine: payload.engine,
        user_id,
        vcpus: payload.vcpus.unwrap_or(CpuCores::try_from(1).unwrap()),
        memory_mib: payload
            .memory_mib
            .unwrap_or(MemoryMb::try_from(512).unwrap()),
        disk_mib: payload.disk_mib.unwrap_or(1024),
        tenant_id: None,
        timeline_id: None,
        tenant_gen: None,
        settings: payload.settings.unwrap_or_default(),
    };

    let db = DatabaseService::create_database(&state, params).await?;

    Ok(Json(DatabaseResponse {
        id: db.id,
        name: db.name,
        engine: db.engine,
        status: format!("{:?}", db.status).to_lowercase(),
        vcpus: db.vcpus.value(),
        memory_mib: db.memory_mib.value(),
        disk_mib: db.disk_mib,
        created_at: db.created_at,
    }))
}

#[rovo::rovo]
pub async fn list_databases(
    auth: AuthUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<DatabaseResponse>>> {
    let user_id = Uuid::parse_str(&auth.user_id)
        .map_err(|_| crate::error::ApiError::Auth("Invalid user ID".to_string()))?;

    let dbs = state
        .ctx
        .database_repo
        .list_databases_by_user(user_id)
        .await
        .map_err(|e| crate::error::ApiError::Internal(e.to_string()))?;

    let response = dbs
        .into_iter()
        .map(|db| DatabaseResponse {
            id: db.id,
            name: db.name,
            engine: db.engine,
            status: format!("{:?}", db.status).to_lowercase(),
            vcpus: db.vcpus.value(),
            memory_mib: db.memory_mib.value(),
            disk_mib: db.disk_mib,
            created_at: db.created_at,
        })
        .collect();

    Ok(Json(response))
}

#[rovo::rovo]
pub async fn delete_database(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<()>> {
    let user_id = Uuid::parse_str(&auth.user_id)
        .map_err(|_| crate::error::ApiError::Auth("Invalid user ID".to_string()))?;

    // Check ownership
    let db = state
        .ctx
        .database_repo
        .get_database(id)
        .await
        .map_err(|e| crate::error::ApiError::Internal(e.to_string()))?
        .ok_or_else(|| crate::error::ApiError::NotFound("Database not found".to_string()))?;

    if db.user_id != user_id {
        return Err(crate::error::ApiError::Forbidden);
    }

    DatabaseService::delete_database(&state, id).await?;

    Ok(Json(()))
}
