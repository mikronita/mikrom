use crate::AppState;
use crate::application::database::{DatabaseConnectionInfo, DatabaseService};
use crate::application::tenant::resolve_tenant_owner_user_id;
use crate::application::vms::VmSnapshot;
use crate::domain::CreateDatabaseParams;
use crate::domain::types::{CpuCores, MemoryMb};
use crate::error::ApiResult;
use crate::infrastructure::auth::extractor::TenantContext;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const SUPPORTED_POSTGRES_VERSIONS: [u16; 3] = [14, 15, 16];

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct CreateDatabaseRequest {
    pub name: String,
    pub engine: String,
    pub postgres_version: Option<u16>,
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
    pub postgres_version: u16,
    pub neon_tenant_id: Option<String>,
    pub neon_timeline_id: Option<String>,
    pub tenant_gen: Option<u32>,
    pub status: String,
    pub vcpus: u32,
    pub memory_mib: u32,
    pub disk_mib: u32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, rovo::schemars::JsonSchema)]
pub struct DatabaseBranchResponse {
    pub database_id: Uuid,
    pub database_name: String,
    pub branch_name: String,
    pub neon_tenant_id: Option<String>,
    pub neon_timeline_id: Option<String>,
    pub tenant_gen: Option<u32>,
    pub status: String,
    pub is_current: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, rovo::schemars::JsonSchema)]
pub struct DatabaseBackupResponse {
    pub database_id: Uuid,
    pub database_name: String,
    pub backup_strategy: String,
    pub recovery_mode: String,
    pub retention_valid: bool,
    pub neon_tenant_id: Option<String>,
    pub neon_timeline_id: Option<String>,
    pub tenant_gen: Option<u32>,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct DatabaseSnapshotNameRequest {
    pub name: String,
}

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct DatabaseRestoreSnapshotRequest {
    pub snapshot_name: String,
}

#[derive(Debug, Serialize, rovo::schemars::JsonSchema)]
pub struct DatabaseSnapshotActionResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Serialize, rovo::schemars::JsonSchema)]
pub struct DatabaseSnapshotListResponse {
    pub success: bool,
    pub message: String,
    pub snapshots: Vec<VmSnapshot>,
}

#[rovo::rovo]
pub async fn create_database(
    tenant_ctx: TenantContext,
    State(state): State<AppState>,
    Json(payload): Json<CreateDatabaseRequest>,
) -> ApiResult<Json<DatabaseResponse>> {
    let tenant_id = tenant_ctx.tenant.id;
    let user_id = resolve_tenant_owner_user_id(&state, tenant_id).await?;
    let postgres_version = validate_postgres_version(payload.postgres_version.unwrap_or(16))?;

    let params = CreateDatabaseParams {
        name: payload.name,
        engine: payload.engine,
        postgres_version,
        user_id,
        tenant_id,
        vcpus: payload.vcpus.unwrap_or(CpuCores::try_from(1).unwrap()),
        memory_mib: payload
            .memory_mib
            .unwrap_or(MemoryMb::try_from(512).unwrap()),
        disk_mib: payload.disk_mib.unwrap_or(1024),
        neon_tenant_id: None,
        neon_timeline_id: None,
        tenant_gen: None,
        settings: payload.settings.unwrap_or_default(),
    };

    let db = DatabaseService::create_database(&state, params).await?;

    Ok(Json(DatabaseResponse {
        id: db.id,
        name: db.name,
        engine: db.engine,
        postgres_version: db.postgres_version,
        neon_tenant_id: db.neon_tenant_id,
        neon_timeline_id: db.neon_timeline_id,
        tenant_gen: db.tenant_gen,
        status: format!("{:?}", db.status).to_lowercase(),
        vcpus: db.vcpus.value(),
        memory_mib: db.memory_mib.value(),
        disk_mib: db.disk_mib,
        created_at: db.created_at,
        updated_at: db.updated_at,
    }))
}

fn validate_postgres_version(version: u16) -> ApiResult<u16> {
    if SUPPORTED_POSTGRES_VERSIONS.contains(&version) {
        Ok(version)
    } else {
        Err(crate::error::ApiError::BadRequest(format!(
            "Unsupported PostgreSQL version {version}. Supported versions are 14, 15, and 16."
        )))
    }
}

#[rovo::rovo]
pub async fn list_databases(
    tenant_ctx: TenantContext,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<DatabaseResponse>>> {
    let dbs = state
        .ctx
        .database_repo
        .list_databases_by_tenant(tenant_ctx.tenant.id)
        .await
        .map_err(|e| crate::error::ApiError::Internal(e.to_string()))?;

    let response = dbs
        .into_iter()
        .map(|db| DatabaseResponse {
            id: db.id,
            name: db.name,
            engine: db.engine,
            postgres_version: db.postgres_version,
            neon_tenant_id: db.neon_tenant_id,
            neon_timeline_id: db.neon_timeline_id,
            tenant_gen: db.tenant_gen,
            status: format!("{:?}", db.status).to_lowercase(),
            vcpus: db.vcpus.value(),
            memory_mib: db.memory_mib.value(),
            disk_mib: db.disk_mib,
            created_at: db.created_at,
            updated_at: db.updated_at,
        })
        .collect();

    Ok(Json(response))
}

#[rovo::rovo]
pub async fn delete_database(
    tenant_ctx: TenantContext,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<()>> {
    // Check ownership
    let db = state
        .ctx
        .database_repo
        .get_database(id)
        .await
        .map_err(|e| crate::error::ApiError::Internal(e.to_string()))?
        .ok_or_else(|| crate::error::ApiError::NotFound("Database not found".to_string()))?;

    if db.tenant_id != tenant_ctx.tenant.id {
        return Err(crate::error::ApiError::Forbidden);
    }

    DatabaseService::delete_database(&state, id).await?;

    Ok(Json(()))
}

#[rovo::rovo]
pub async fn get_database_connection(
    tenant_ctx: TenantContext,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<DatabaseConnectionInfo>> {
    let connection = DatabaseService::get_connection_info(&state, id, tenant_ctx.tenant.id).await?;
    Ok(Json(connection))
}

#[rovo::rovo]
pub async fn list_database_branches(
    tenant_ctx: TenantContext,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<DatabaseBranchResponse>>> {
    let db = state
        .ctx
        .database_repo
        .get_database(id)
        .await
        .map_err(|e| crate::error::ApiError::Internal(e.to_string()))?
        .ok_or_else(|| crate::error::ApiError::NotFound("Database not found".to_string()))?;

    if db.tenant_id != tenant_ctx.tenant.id {
        return Err(crate::error::ApiError::Forbidden);
    }

    Ok(Json(vec![to_database_branch_response(db)]))
}

fn to_database_branch_response(db: crate::domain::Database) -> DatabaseBranchResponse {
    DatabaseBranchResponse {
        database_id: db.id,
        database_name: db.name,
        branch_name: "current".to_string(),
        neon_tenant_id: db.neon_tenant_id,
        neon_timeline_id: db.neon_timeline_id,
        tenant_gen: db.tenant_gen,
        status: format!("{:?}", db.status).to_lowercase(),
        is_current: true,
        created_at: db.created_at,
        updated_at: db.updated_at,
    }
}

#[rovo::rovo]
pub async fn get_database_backups(
    tenant_ctx: TenantContext,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<DatabaseBackupResponse>> {
    let db = state
        .ctx
        .database_repo
        .get_database(id)
        .await
        .map_err(|e| crate::error::ApiError::Internal(e.to_string()))?
        .ok_or_else(|| crate::error::ApiError::NotFound("Database not found".to_string()))?;

    if db.tenant_id != tenant_ctx.tenant.id {
        return Err(crate::error::ApiError::Forbidden);
    }

    let retention_valid = DatabaseService::validate_tenant_retention(
        &state,
        db.neon_tenant_id.as_deref().unwrap_or(""),
        db.tenant_gen.unwrap_or(1),
    )
    .await;

    Ok(Json(to_database_backup_response(&db, retention_valid)))
}

#[rovo::rovo]
pub async fn list_database_snapshots(
    tenant_ctx: TenantContext,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<DatabaseSnapshotListResponse>> {
    let (success, message, snapshots) =
        DatabaseService::list_backup_snapshots(&state, id, tenant_ctx.tenant.id).await?;

    Ok(Json(DatabaseSnapshotListResponse {
        success,
        message,
        snapshots,
    }))
}

#[rovo::rovo]
pub async fn create_database_snapshot(
    tenant_ctx: TenantContext,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<DatabaseSnapshotNameRequest>,
) -> ApiResult<(StatusCode, Json<DatabaseSnapshotActionResponse>)> {
    let (success, message) =
        DatabaseService::create_backup_snapshot(&state, id, tenant_ctx.tenant.id, payload.name)
            .await?;

    Ok((
        StatusCode::CREATED,
        Json(DatabaseSnapshotActionResponse { success, message }),
    ))
}

#[rovo::rovo]
pub async fn restore_database_snapshot(
    tenant_ctx: TenantContext,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<DatabaseRestoreSnapshotRequest>,
) -> ApiResult<Json<DatabaseSnapshotActionResponse>> {
    let (success, message) = DatabaseService::restore_backup_snapshot(
        &state,
        id,
        tenant_ctx.tenant.id,
        payload.snapshot_name,
    )
    .await?;

    Ok(Json(DatabaseSnapshotActionResponse { success, message }))
}

#[rovo::rovo]
pub async fn delete_database_snapshot(
    tenant_ctx: TenantContext,
    State(state): State<AppState>,
    Path((id, snapshot_name)): Path<(Uuid, String)>,
) -> ApiResult<Json<DatabaseSnapshotActionResponse>> {
    let (success, message) =
        DatabaseService::delete_backup_snapshot(&state, id, tenant_ctx.tenant.id, snapshot_name)
            .await?;

    Ok(Json(DatabaseSnapshotActionResponse { success, message }))
}

fn to_database_backup_response(
    db: &crate::domain::Database,
    retention_valid: bool,
) -> DatabaseBackupResponse {
    let has_neon_branch = db
        .neon_tenant_id
        .as_deref()
        .is_some_and(|tenant_id| !tenant_id.starts_with("pending-"))
        && db
            .neon_timeline_id
            .as_deref()
            .is_some_and(|timeline_id| !timeline_id.starts_with("pending-"));

    let backup_strategy = if has_neon_branch {
        "continuous".to_string()
    } else {
        "pending".to_string()
    };

    let recovery_mode = if has_neon_branch && retention_valid {
        "point-in-time recovery available".to_string()
    } else if has_neon_branch {
        "branch provisioned, retention not yet validated".to_string()
    } else {
        "awaiting Neon provisioning".to_string()
    };

    DatabaseBackupResponse {
        database_id: db.id,
        database_name: db.name.clone(),
        backup_strategy,
        recovery_mode,
        retention_valid,
        neon_tenant_id: db.neon_tenant_id.clone(),
        neon_timeline_id: db.neon_timeline_id.clone(),
        tenant_gen: db.tenant_gen,
        status: format!("{:?}", db.status).to_lowercase(),
        created_at: db.created_at,
        updated_at: db.updated_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CpuCores, Database, DatabaseStatus, MemoryMb};
    use chrono::Utc;
    use uuid::Uuid;

    #[test]
    fn validate_postgres_version_accepts_supported_versions() {
        for version in SUPPORTED_POSTGRES_VERSIONS {
            assert_eq!(validate_postgres_version(version).unwrap(), version);
        }
    }

    #[test]
    fn validate_postgres_version_rejects_unsupported_versions() {
        let err = validate_postgres_version(13).unwrap_err();
        match err {
            crate::error::ApiError::BadRequest(message) => {
                assert_eq!(
                    message,
                    "Unsupported PostgreSQL version 13. Supported versions are 14, 15, and 16."
                );
            },
            other => panic!("expected bad request error, got {other:?}"),
        }
    }

    #[test]
    fn database_branch_response_reflects_current_branch_state() {
        let db = Database {
            id: Uuid::new_v4(),
            name: "orders".to_string(),
            engine: "neon".to_string(),
            postgres_version: 16,
            tenant_id: Uuid::new_v4(),
            vcpus: CpuCores::new(1).unwrap(),
            memory_mib: MemoryMb::new(512).unwrap(),
            disk_mib: 1024,
            neon_tenant_id: Some("11111111111111111111111111111111".to_string()),
            neon_timeline_id: Some("22222222222222222222222222222222".to_string()),
            tenant_gen: Some(1),
            settings: std::collections::HashMap::new(),
            status: DatabaseStatus::Running,
            active_deployment_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let branch = to_database_branch_response(db);
        assert!(branch.is_current);
        assert_eq!(branch.branch_name, "current");
        assert_eq!(branch.status, "running");
        assert_eq!(
            branch.neon_tenant_id.as_deref(),
            Some("11111111111111111111111111111111")
        );
        assert_eq!(
            branch.neon_timeline_id.as_deref(),
            Some("22222222222222222222222222222222")
        );
        assert_eq!(branch.tenant_gen, Some(1));
    }

    #[test]
    fn database_backup_response_reflects_retention_state() {
        let db = Database {
            id: Uuid::new_v4(),
            name: "orders".to_string(),
            engine: "neon".to_string(),
            postgres_version: 16,
            tenant_id: Uuid::new_v4(),
            vcpus: CpuCores::new(1).unwrap(),
            memory_mib: MemoryMb::new(512).unwrap(),
            disk_mib: 1024,
            neon_tenant_id: Some("11111111111111111111111111111111".to_string()),
            neon_timeline_id: Some("22222222222222222222222222222222".to_string()),
            tenant_gen: Some(1),
            settings: std::collections::HashMap::new(),
            status: DatabaseStatus::Running,
            active_deployment_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let backup = to_database_backup_response(&db, true);
        assert_eq!(backup.backup_strategy, "continuous");
        assert_eq!(backup.recovery_mode, "point-in-time recovery available");
        assert!(backup.retention_valid);
        assert_eq!(backup.status, "running");
    }

    #[test]
    fn database_backup_response_reflects_pending_provisioning() {
        let db = Database {
            id: Uuid::new_v4(),
            name: "orders".to_string(),
            engine: "neon".to_string(),
            postgres_version: 16,
            tenant_id: Uuid::new_v4(),
            vcpus: CpuCores::new(1).unwrap(),
            memory_mib: MemoryMb::new(512).unwrap(),
            disk_mib: 1024,
            neon_tenant_id: None,
            neon_timeline_id: None,
            tenant_gen: None,
            settings: std::collections::HashMap::new(),
            status: DatabaseStatus::Pending,
            active_deployment_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let backup = to_database_backup_response(&db, false);
        assert_eq!(backup.backup_strategy, "pending");
        assert_eq!(backup.recovery_mode, "awaiting Neon provisioning");
        assert!(!backup.retention_valid);
        assert_eq!(backup.status, "pending");
    }
}
