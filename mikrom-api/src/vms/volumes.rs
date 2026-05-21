use crate::error::{ApiError, ApiResult};
use crate::models::volume::{Volume, VolumeSnapshot};
use crate::repositories::volume_repository::{CreateSnapshotParams, CreateVolumeParams};
use crate::workspace::{WorkspaceEvent, WorkspaceEventKind};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct CreateVolumeRequest {
    pub name: String,
    pub size_mib: i32,
    #[serde(default = "default_mount_point")]
    pub mount_point: String,
    #[serde(default)]
    pub access_mode: i32,
}

fn default_mount_point() -> String {
    "/data".to_string()
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
    Path(app_id): Path<Uuid>,
    Json(req): Json<CreateVolumeRequest>,
) -> ApiResult<(StatusCode, Json<Volume>)> {
    let app = state
        .app_repo
        .get_app(app_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("App not found".to_string()))?;

    if app.user_id.to_string() != auth.user_id {
        return Err(ApiError::Forbidden);
    }

    let pool_name = format!("user_{}_volumes", app.user_id.to_string().replace('-', "_"));

    // Validate mount point
    if !req.mount_point.starts_with('/') || req.mount_point.contains("..") {
        return Err(ApiError::BadRequest(
            "Mount point must be an absolute path and cannot contain ..".to_string(),
        ));
    }

    let forbidden_paths = [
        "/", "/etc", "/proc", "/sys", "/dev", "/bin", "/sbin", "/lib", "/usr", "/root", "/boot",
        "/var", "/tmp", "/home", "/run",
    ];
    for path in forbidden_paths {
        if req.mount_point == path
            || (path != "/" && req.mount_point.starts_with(&format!("{}/", path)))
        {
            return Err(ApiError::BadRequest(format!(
                "Mount point {} is reserved by the system",
                req.mount_point
            )));
        }
    }

    let volume = state
        .volume_repo
        .create_volume(CreateVolumeParams {
            app_id,
            user_id: app.user_id,
            name: req.name,
            size_mib: req.size_mib,
            pool_name: pool_name.clone(),
            mount_point: req.mount_point,
            access_mode: req.access_mode,
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
        .request("mikrom.scheduler.create_volume", nats_req)
        .await
        .map_err(|e| ApiError::Scheduler(e.to_string()))?;

    if !resp.success {
        return Err(ApiError::Scheduler(resp.message));
    }

    // Emit workspace event
    if let Err(e) = state.workspace_events.send(WorkspaceEvent {
        kind: WorkspaceEventKind::VolumeChanged,
        user_id: Some(app.user_id),
        app_id: Some(app.id),
        app_name: Some(app.name),
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
) -> ApiResult<Json<Vec<Volume>>> {
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

    // 1. Find where the app is currently running to route the snapshot command
    let app = state
        .app_repo
        .get_app(volume.app_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("App not found".to_string()))?;

    let host_id = if let Some(dep_id) = app.active_deployment_id {
        if let Some(deployment) = state.app_repo.get_deployment(dep_id).await? {
            if let Some(job_id) = deployment.job_id {
                if job_id.starts_with("temp-") {
                    None
                } else {
                    use mikrom_proto::scheduler::{AppStatusRequest, AppStatusResponse};
                    let inner: AppStatusResponse = state
                        .nats
                        .request(
                            "mikrom.scheduler.get_job",
                            AppStatusRequest {
                                job_id,
                                user_id: auth.user_id.clone(),
                            },
                        )
                        .await
                        .map_err(|e| {
                            ApiError::Scheduler(format!("Scheduler request failed: {e}"))
                        })?;

                    if inner.host_id.is_empty() {
                        None
                    } else {
                        Some(inner.host_id)
                    }
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    use mikrom_proto::scheduler::{
        CreateSnapshotRequest as SchedSnapReq, CreateSnapshotResponse as SchedSnapRes,
    };

    let nats_req = SchedSnapReq {
        volume_id: volume_id.to_string(),
        snapshot_name: req.name,
        pool_name: volume.pool_name,
        host_id: host_id.unwrap_or_default(),
    };

    let scheduler_res: SchedSnapRes = state
        .nats
        .with_timeout(std::time::Duration::from_secs(30))
        .request("mikrom.scheduler.create_snapshot", nats_req)
        .await
        .map_err(|e| ApiError::Scheduler(format!("Scheduler request failed: {e}")))?;

    if !scheduler_res.success {
        return Err(ApiError::Scheduler(scheduler_res.message));
    }

    // Emit workspace event
    if let Err(e) = state.workspace_events.send(WorkspaceEvent {
        kind: WorkspaceEventKind::SnapshotChanged,
        user_id: Some(volume.user_id),
        app_id: Some(volume.app_id),
        app_name: None,
        deployment_id: None,
        volume_id: Some(volume.id),
        resource_id: Some(snapshot.id.to_string()),
    }) {
        tracing::warn!(error = %e, "Failed to broadcast SnapshotChanged event");
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

    // 1. Tell Scheduler to delete physical volume in Ceph
    use mikrom_proto::scheduler::{DeleteVolumeRequest, DeleteVolumeResponse};

    let nats_req = DeleteVolumeRequest {
        volume_id: volume_id.to_string(),
        pool_name: volume.pool_name,
        host_id: String::new(), // Scheduler picks any worker
    };

    let scheduler_res: DeleteVolumeResponse = state
        .nats
        .with_timeout(std::time::Duration::from_secs(30))
        .request("mikrom.scheduler.delete_volume", nats_req)
        .await
        .map_err(|e| ApiError::Scheduler(format!("Scheduler request failed: {e}")))?;

    if !scheduler_res.success {
        return Err(ApiError::Scheduler(scheduler_res.message));
    }

    // 2. Delete from DB
    state.volume_repo.delete_volume(volume_id).await?;

    // Emit workspace event
    if let Err(e) = state.workspace_events.send(WorkspaceEvent {
        kind: WorkspaceEventKind::VolumeChanged,
        user_id: Some(volume.user_id),
        app_id: Some(volume.app_id),
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

    // 1. Tell Scheduler to restore the snapshot in Ceph
    use mikrom_proto::scheduler::{
        RestoreSnapshotRequest as SchedRestoreReq, RestoreSnapshotResponse as SchedRestoreRes,
    };

    let nats_req = SchedRestoreReq {
        volume_id: volume_id.to_string(),
        snapshot_name: req.snapshot_name,
        pool_name: volume.pool_name,
        host_id: String::new(), // Scheduler picks any worker
    };

    let scheduler_res: SchedRestoreRes = state
        .nats
        .with_timeout(std::time::Duration::from_secs(30))
        .request("mikrom.scheduler.restore_snapshot", nats_req)
        .await
        .map_err(|e| ApiError::Scheduler(format!("Scheduler request failed: {e}")))?;

    if !scheduler_res.success {
        return Err(ApiError::Scheduler(scheduler_res.message));
    }

    // Emit workspace event
    if let Err(e) = state.workspace_events.send(WorkspaceEvent {
        kind: WorkspaceEventKind::VolumeChanged,
        user_id: Some(volume.user_id),
        app_id: Some(volume.app_id),
        app_name: None,
        deployment_id: None,
        volume_id: Some(volume_id),
        resource_id: Some(volume_id.to_string()),
    }) {
        tracing::warn!(error = %e, "Failed to broadcast VolumeChanged event");
    }

    Ok(StatusCode::OK)
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

    // 1. Tell Scheduler to delete physical snapshot in Ceph
    use mikrom_proto::scheduler::{DeleteSnapshotRequest, DeleteSnapshotResponse};

    let nats_req = DeleteSnapshotRequest {
        volume_id: volume.id.to_string(),
        snapshot_name: snapshot.name,
        pool_name: volume.pool_name,
        host_id: String::new(), // Scheduler picks any worker
    };

    let scheduler_res: DeleteSnapshotResponse = state
        .nats
        .with_timeout(std::time::Duration::from_secs(30))
        .request("mikrom.scheduler.delete_snapshot", nats_req)
        .await
        .map_err(|e| ApiError::Scheduler(format!("Scheduler request failed: {e}")))?;

    if !scheduler_res.success {
        return Err(ApiError::Scheduler(scheduler_res.message));
    }

    // 2. Delete from DB
    state.volume_repo.delete_snapshot(snapshot_id).await?;

    // Emit workspace event
    if let Err(e) = state.workspace_events.send(WorkspaceEvent {
        kind: WorkspaceEventKind::SnapshotChanged,
        user_id: Some(snapshot.user_id),
        app_id: Some(volume.app_id),
        app_name: None,
        deployment_id: None,
        volume_id: Some(volume.id),
        resource_id: Some(snapshot_id.to_string()),
    }) {
        tracing::warn!(error = %e, "Failed to broadcast SnapshotChanged event");
    }

    Ok(StatusCode::NO_CONTENT)
}

#[rovo::rovo]
pub async fn clone_volume_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(volume_id): Path<Uuid>,
    Json(req): Json<CloneVolumeRequest>,
) -> ApiResult<(StatusCode, Json<Volume>)> {
    let source_volume = state
        .volume_repo
        .get_volume(volume_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Source volume not found".to_string()))?;

    if source_volume.user_id.to_string() != auth.user_id {
        return Err(ApiError::Forbidden);
    }

    // 1. Create target volume in DB
    let target_volume = state
        .volume_repo
        .create_volume(CreateVolumeParams {
            app_id: source_volume.app_id,
            user_id: source_volume.user_id,
            name: req.name,
            size_mib: source_volume.size_mib, // Clones usually keep the same size initially
            pool_name: source_volume.pool_name.clone(),
            mount_point: source_volume.mount_point.clone(),
            access_mode: source_volume.access_mode,
        })
        .await?;

    // 2. Tell Scheduler to clone physical volume in Ceph
    use mikrom_proto::scheduler::{
        CloneVolumeRequest as SchedCloneReq, CloneVolumeResponse as SchedCloneRes,
    };

    let nats_req = SchedCloneReq {
        source_volume_id: source_volume.id.to_string(),
        snapshot_name: req.snapshot_name,
        target_volume_id: target_volume.id.to_string(),
        pool_name: source_volume.pool_name,
        host_id: String::new(), // Scheduler picks any worker
    };

    let scheduler_res: SchedCloneRes = state
        .nats
        .with_timeout(std::time::Duration::from_secs(30))
        .request("mikrom.scheduler.clone_volume", nats_req)
        .await
        .map_err(|e| ApiError::Scheduler(format!("Scheduler request failed: {e}")))?;

    if !scheduler_res.success {
        // Rollback DB entry if physical clone fails
        if let Err(e) = state.volume_repo.delete_volume(target_volume.id).await {
            tracing::error!(volume_id = %target_volume.id, error = %e, "Failed to rollback volume DB entry after physical clone failure");
        }
        return Err(ApiError::Scheduler(scheduler_res.message));
    }

    // Emit workspace event
    if let Err(e) = state.workspace_events.send(WorkspaceEvent {
        kind: WorkspaceEventKind::VolumeChanged,
        user_id: Some(target_volume.user_id),
        app_id: Some(target_volume.app_id),
        app_name: None,
        deployment_id: None,
        volume_id: Some(target_volume.id),
        resource_id: Some(target_volume.id.to_string()),
    }) {
        tracing::warn!(error = %e, "Failed to broadcast VolumeChanged event");
    }

    Ok((StatusCode::CREATED, Json(target_volume)))
}
