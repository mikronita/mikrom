use crate::error::{ApiError, ApiResult};
use crate::models::volume::{
    AppVolume, AttachedVolume, Volume, VolumeSnapshot, VolumeWithAttachments,
};
use crate::repositories::volume_repository::{CreateSnapshotParams, CreateVolumeParams};
use crate::workspace::{WorkspaceEvent, WorkspaceEventKind};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct CreateVolumeRequest {
    pub name: String,
    pub size_mib: i32,
}

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct AttachVolumeRequest {
    pub volume_id: Uuid,
    #[serde(default = "default_mount_point")]
    pub mount_point: String,
    #[serde(default)]
    pub access_mode: i32,
}

fn default_mount_point() -> String {
    "/data".to_string()
}

fn validate_mount_point(mount_point: &str) -> ApiResult<()> {
    if !mount_point.starts_with('/') || mount_point.contains("..") {
        return Err(ApiError::BadRequest(
            "Mount point must be an absolute path and cannot contain ..".to_string(),
        ));
    }

    let forbidden_paths = [
        "/", "/etc", "/proc", "/sys", "/dev", "/bin", "/sbin", "/lib", "/usr", "/root", "/boot",
        "/var", "/tmp", "/home", "/run",
    ];
    for path in forbidden_paths {
        if mount_point == path || (path != "/" && mount_point.starts_with(&format!("{}/", path))) {
            return Err(ApiError::BadRequest(format!(
                "Mount point {} is reserved by the system",
                mount_point
            )));
        }
    }

    Ok(())
}

fn has_duplicate_mount_point(volumes: &[AttachedVolume], mount_point: &str) -> bool {
    volumes
        .iter()
        .any(|volume| volume.mount_point == mount_point)
}

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct CreateSnapshotRequest {
    pub name: String,
}

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct RestoreSnapshotRequest {
    pub snapshot_name: String,
}

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct CloneVolumeRequest {
    pub name: String,
    pub snapshot_name: String,
}

#[rovo::rovo]
pub async fn create_volume_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Json(req): Json<CreateVolumeRequest>,
) -> ApiResult<(StatusCode, Json<Volume>)> {
    let user_id = Uuid::parse_str(&auth.user_id)
        .map_err(|_| ApiError::Internal("Invalid user id".to_string()))?;

    let pool_name = format!("user_{}_volumes", user_id.to_string().replace('-', "_"));

    let volume = state
        .volume_repo
        .create_volume(CreateVolumeParams {
            user_id,
            name: req.name,
            size_mib: req.size_mib,
            pool_name: pool_name.clone(),
        })
        .await?;

    // Physically create the volume via Scheduler
    let nats_req = mikrom_proto::scheduler::CreateVolumeRequest {
        volume_id: volume.id.to_string(),
        size_mib: volume.size_mib as u32,
        pool_name: pool_name.clone(),
        host_id: "".to_string(), // Scheduler will pick one
    };

    let resp: mikrom_proto::scheduler::CreateVolumeResponse = state
        .nats
        .with_timeout(Duration::from_secs(5))
        .request("mikrom.scheduler.create_volume", nats_req)
        .await
        .map_err(|e| ApiError::Scheduler(e.to_string()))?;

    if !resp.success {
        return Err(ApiError::Scheduler(resp.message));
    }

    // Emit workspace event
    if let Err(e) = state.workspace_events.send(WorkspaceEvent {
        kind: WorkspaceEventKind::VolumeChanged,
        user_id: Some(user_id),
        app_id: None,
        app_name: None,
        deployment_id: None,
        volume_id: Some(volume.id),
        resource_id: Some(volume.id.to_string()),
    }) {
        tracing::warn!(error = %e, "Failed to broadcast VolumeChanged event");
    }

    Ok((StatusCode::CREATED, Json(volume)))
}

#[rovo::rovo]
pub async fn list_volumes_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(app_id): Path<Uuid>,
) -> ApiResult<Json<Vec<AttachedVolume>>> {
    let app = state
        .app_repo
        .get_app(app_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("App not found".to_string()))?;

    if app.user_id.to_string() != auth.user_id {
        return Err(ApiError::Forbidden);
    }

    let volumes = state.volume_repo.list_volumes_by_app(app_id).await?;
    Ok(Json(volumes))
}

#[rovo::rovo]
pub async fn list_all_volumes_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
) -> ApiResult<Json<Vec<VolumeWithAttachments>>> {
    let user_id = uuid::Uuid::parse_str(&auth.user_id)
        .map_err(|_| ApiError::Internal("Invalid user id".to_string()))?;
    let volumes = state.volume_repo.list_volumes_by_user(user_id).await?;
    Ok(Json(volumes))
}

#[rovo::rovo]
pub async fn attach_volume_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(app_id): Path<Uuid>,
    Json(req): Json<AttachVolumeRequest>,
) -> ApiResult<(StatusCode, Json<AppVolume>)> {
    let app = state
        .app_repo
        .get_app(app_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("App not found".to_string()))?;

    if app.user_id.to_string() != auth.user_id {
        return Err(ApiError::Forbidden);
    }

    let volume = state
        .volume_repo
        .get_volume(req.volume_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Volume not found".to_string()))?;

    if volume.user_id != app.user_id {
        return Err(ApiError::Forbidden);
    }

    validate_mount_point(&req.mount_point)?;

    let existing_volumes = state.volume_repo.list_volumes_by_app(app_id).await?;
    if has_duplicate_mount_point(&existing_volumes, &req.mount_point) {
        return Err(ApiError::BadRequest(format!(
            "Mount point {} is already in use for this application",
            req.mount_point
        )));
    }

    let app_volume = state
        .volume_repo
        .attach_volume_to_app(app_id, req.volume_id, req.mount_point, req.access_mode)
        .await?;

    // Emit workspace event
    if let Err(e) = state.workspace_events.send(WorkspaceEvent {
        kind: WorkspaceEventKind::VolumeChanged,
        user_id: Some(app.user_id),
        app_id: Some(app.id),
        app_name: Some(app.name),
        deployment_id: None,
        volume_id: Some(req.volume_id),
        resource_id: Some(req.volume_id.to_string()),
    }) {
        tracing::warn!(error = %e, "Failed to broadcast VolumeChanged event");
    }

    Ok((StatusCode::OK, Json(app_volume)))
}

#[rovo::rovo]
pub async fn detach_volume_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path((app_id, volume_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    let app = state
        .app_repo
        .get_app(app_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("App not found".to_string()))?;

    if app.user_id.to_string() != auth.user_id {
        return Err(ApiError::Forbidden);
    }

    let detached = state
        .volume_repo
        .detach_volume_from_app(app_id, volume_id)
        .await?;

    if !detached {
        return Err(ApiError::NotFound("Attachment not found".to_string()));
    }

    // Emit workspace event
    if let Err(e) = state.workspace_events.send(WorkspaceEvent {
        kind: WorkspaceEventKind::VolumeChanged,
        user_id: Some(app.user_id),
        app_id: Some(app.id),
        app_name: Some(app.name),
        deployment_id: None,
        volume_id: Some(volume_id),
        resource_id: Some(volume_id.to_string()),
    }) {
        tracing::warn!(error = %e, "Failed to broadcast VolumeChanged event");
    }

    Ok(StatusCode::NO_CONTENT)
}

#[rovo::rovo]
pub async fn list_snapshots_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(volume_id): Path<Uuid>,
) -> ApiResult<Json<Vec<VolumeSnapshot>>> {
    let volume = state
        .volume_repo
        .get_volume(volume_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Volume not found".to_string()))?;

    if volume.user_id.to_string() != auth.user_id {
        return Err(ApiError::Forbidden);
    }

    let snapshots = state
        .volume_repo
        .list_snapshots_by_volume(volume_id)
        .await?;
    Ok(Json(snapshots))
}

#[rovo::rovo]
pub async fn create_snapshot_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(volume_id): Path<Uuid>,
    Json(req): Json<CreateSnapshotRequest>,
) -> ApiResult<(StatusCode, Json<VolumeSnapshot>)> {
    let volume = state
        .volume_repo
        .get_volume(volume_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Volume not found".to_string()))?;

    if volume.user_id.to_string() != auth.user_id {
        return Err(ApiError::Forbidden);
    }

    let snapshot = state
        .volume_repo
        .create_snapshot(CreateSnapshotParams {
            volume_id,
            user_id: volume.user_id,
            name: req.name.clone(),
        })
        .await?;

    // Physically create the snapshot via Scheduler
    let nats_req = mikrom_proto::scheduler::CreateSnapshotRequest {
        volume_id: volume.id.to_string(),
        snapshot_name: snapshot.name.clone(),
        pool_name: volume.pool_name.clone(),
        host_id: String::new(),
    };

    let resp: mikrom_proto::scheduler::CreateSnapshotResponse = state
        .nats
        .with_timeout(Duration::from_secs(5))
        .request("mikrom.scheduler.create_snapshot", nats_req)
        .await
        .map_err(|e| ApiError::Scheduler(e.to_string()))?;

    if !resp.success {
        return Err(ApiError::Scheduler(resp.message));
    }

    Ok((StatusCode::CREATED, Json(snapshot)))
}

#[rovo::rovo]
pub async fn delete_volume_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(volume_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let volume = state
        .volume_repo
        .get_volume(volume_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Volume not found".to_string()))?;

    if volume.user_id.to_string() != auth.user_id {
        return Err(ApiError::Forbidden);
    }

    // Check for snapshots before deletion
    let snapshots = state
        .volume_repo
        .list_snapshots_by_volume(volume_id)
        .await?;

    if !snapshots.is_empty() {
        return Err(ApiError::BadRequest(
            "Cannot delete volume because it has snapshots. Please delete all snapshots first."
                .to_string(),
        ));
    }

    // Check if volume is attached to any app
    let is_attached = state.volume_repo.is_volume_attached(volume_id).await?;

    if is_attached {
        return Err(ApiError::BadRequest(
            "Cannot delete volume because it is attached to one or more applications. Please detach it first."
                .to_string(),
        ));
    }

    // Physically delete the volume via Scheduler
    let nats_req = mikrom_proto::scheduler::DeleteVolumeRequest {
        volume_id: volume_id.to_string(),
        pool_name: volume.pool_name.clone(),
        host_id: String::new(),
    };

    let resp: mikrom_proto::scheduler::DeleteVolumeResponse = state
        .nats
        .with_timeout(Duration::from_secs(5))
        .request("mikrom.scheduler.delete_volume", nats_req)
        .await
        .map_err(|e| ApiError::Scheduler(e.to_string()))?;

    if !resp.success {
        return Err(ApiError::Scheduler(resp.message));
    }

    state.volume_repo.delete_volume(volume_id).await?;

    // Emit workspace event
    if let Err(e) = state.workspace_events.send(WorkspaceEvent {
        kind: WorkspaceEventKind::VolumeChanged,
        user_id: Some(volume.user_id),
        app_id: None,
        app_name: None,
        deployment_id: None,
        volume_id: Some(volume_id),
        resource_id: Some(volume_id.to_string()),
    }) {
        tracing::warn!(error = %e, "Failed to broadcast VolumeChanged event");
    }

    Ok(StatusCode::NO_CONTENT)
}

#[rovo::rovo]
pub async fn delete_snapshot_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(snapshot_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let snapshot = state
        .volume_repo
        .get_snapshot(snapshot_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Snapshot not found".to_string()))?;

    if snapshot.user_id.to_string() != auth.user_id {
        return Err(ApiError::Forbidden);
    }

    let volume = state
        .volume_repo
        .get_volume(snapshot.volume_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Volume not found".to_string()))?;

    // Physically delete the snapshot via Scheduler
    let nats_req = mikrom_proto::scheduler::DeleteSnapshotRequest {
        volume_id: snapshot.volume_id.to_string(),
        snapshot_name: snapshot.name.clone(),
        pool_name: volume.pool_name.clone(),
        host_id: String::new(),
    };

    let resp: mikrom_proto::scheduler::DeleteSnapshotResponse = state
        .nats
        .with_timeout(Duration::from_secs(5))
        .request("mikrom.scheduler.delete_snapshot", nats_req)
        .await
        .map_err(|e| ApiError::Scheduler(e.to_string()))?;

    if !resp.success {
        return Err(ApiError::Scheduler(resp.message));
    }

    state.volume_repo.delete_snapshot(snapshot_id).await?;

    Ok(StatusCode::NO_CONTENT)
}

#[rovo::rovo]
pub async fn restore_snapshot_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(volume_id): Path<Uuid>,
    Json(req): Json<RestoreSnapshotRequest>,
) -> ApiResult<StatusCode> {
    let volume = state
        .volume_repo
        .get_volume(volume_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Volume not found".to_string()))?;

    if volume.user_id.to_string() != auth.user_id {
        return Err(ApiError::Forbidden);
    }

    // Physically restore the snapshot via Scheduler
    let nats_req = mikrom_proto::scheduler::RestoreSnapshotRequest {
        volume_id: volume_id.to_string(),
        snapshot_name: req.snapshot_name,
        pool_name: volume.pool_name.clone(),
        host_id: String::new(),
    };

    let resp: mikrom_proto::scheduler::RestoreSnapshotResponse = state
        .nats
        .with_timeout(Duration::from_secs(5))
        .request("mikrom.scheduler.restore_snapshot", nats_req)
        .await
        .map_err(|e| ApiError::Scheduler(e.to_string()))?;

    if !resp.success {
        return Err(ApiError::Scheduler(resp.message));
    }

    Ok(StatusCode::OK)
}

#[rovo::rovo]
pub async fn clone_volume_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(volume_id): Path<Uuid>,
    Json(req): Json<CloneVolumeRequest>,
) -> ApiResult<(StatusCode, Json<Volume>)> {
    let volume = state
        .volume_repo
        .get_volume(volume_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Volume not found".to_string()))?;

    if volume.user_id.to_string() != auth.user_id {
        return Err(ApiError::Forbidden);
    }

    let user_id = Uuid::parse_str(&auth.user_id)
        .map_err(|_| ApiError::Internal("Invalid user id".to_string()))?;

    let pool_name = format!("user_{}_volumes", user_id.to_string().replace('-', "_"));

    // Create the record for the new cloned volume
    let new_volume = state
        .volume_repo
        .create_volume(CreateVolumeParams {
            user_id,
            name: req.name.clone(),
            size_mib: volume.size_mib,
            pool_name: pool_name.clone(),
        })
        .await?;

    // Physically clone the volume via Scheduler
    let nats_req = mikrom_proto::scheduler::CloneVolumeRequest {
        source_volume_id: volume_id.to_string(),
        snapshot_name: req.snapshot_name,
        target_volume_id: new_volume.id.to_string(),
        pool_name: pool_name.clone(),
        host_id: String::new(),
    };

    let resp: mikrom_proto::scheduler::CloneVolumeResponse = state
        .nats
        .with_timeout(Duration::from_secs(5))
        .request("mikrom.scheduler.clone_volume", nats_req)
        .await
        .map_err(|e| ApiError::Scheduler(e.to_string()))?;

    if !resp.success {
        // Rollback DB record if physical clone fails
        state.volume_repo.delete_volume(new_volume.id).await?;
        return Err(ApiError::Scheduler(resp.message));
    }

    // Emit workspace event
    if let Err(e) = state.workspace_events.send(WorkspaceEvent {
        kind: WorkspaceEventKind::VolumeChanged,
        user_id: Some(user_id),
        app_id: None,
        app_name: None,
        deployment_id: None,
        volume_id: Some(new_volume.id),
        resource_id: Some(new_volume.id.to_string()),
    }) {
        tracing::warn!(error = %e, "Failed to broadcast VolumeChanged event");
    }

    Ok((StatusCode::CREATED, Json(new_volume)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_mount_point_rejects_duplicate_paths() {
        let volumes = vec![AttachedVolume {
            volume: Volume {
                id: Uuid::new_v4(),
                user_id: Uuid::new_v4(),
                name: "vol-a".to_string(),
                size_mib: 1024,
                pool_name: "pool-a".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            mount_point: "/data".to_string(),
            access_mode: 0,
        }];

        assert!(has_duplicate_mount_point(&volumes, "/data"));
        assert!(!has_duplicate_mount_point(&volumes, "/cache"));
    }

    #[test]
    fn validate_mount_point_rejects_reserved_and_relative_paths() {
        assert!(validate_mount_point("data").is_err());
        assert!(validate_mount_point("/etc/app").is_err());
        assert!(validate_mount_point("/data").is_ok());
    }
}
