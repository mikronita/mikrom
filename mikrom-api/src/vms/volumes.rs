use crate::error::{ApiError, ApiResult};
use crate::models::volume::{Volume, VolumeSnapshot};
use crate::repositories::volume_repository::{CreateSnapshotParams, CreateVolumeParams};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateVolumeRequest {
    pub name: String,
    pub size_mib: i32,
    #[serde(default = "default_mount_point")]
    pub mount_point: String,
}

fn default_mount_point() -> String {
    "/data".to_string()
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateSnapshotRequest {
    pub name: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RestoreSnapshotRequest {
    pub snapshot_name: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CloneVolumeRequest {
    pub name: String,
    pub snapshot_name: String,
}

#[utoipa::path(
    post,
    path = "/v1/apps/{app_id}/volumes",
    params(
        ("app_id" = Uuid, Path, description = "Application ID")
    ),
    request_body = CreateVolumeRequest,
    responses(
        (status = 201, description = "Volume created", body = Volume),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "App not found")
    ),
    tag = "volume",
    security(("jwt" = []))
)]
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

    Ok((StatusCode::CREATED, Json(volume)))
}

#[utoipa::path(
    get,
    path = "/v1/apps/{app_id}/volumes",
    params(
        ("app_id" = Uuid, Path, description = "Application ID")
    ),
    responses(
        (status = 200, description = "List of volumes", body = [Volume]),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "App not found")
    ),
    tag = "volume",
    security(("jwt" = []))
)]
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

#[utoipa::path(
    get,
    path = "/v1/volumes/{volume_id}/snapshots",
    params(
        ("volume_id" = Uuid, Path, description = "Volume ID")
    ),
    responses(
        (status = 200, description = "List of snapshots", body = [VolumeSnapshot]),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Volume not found")
    ),
    tag = "volume",
    security(("jwt" = []))
)]
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

#[utoipa::path(
    post,
    path = "/v1/volumes/{volume_id}/snapshots",
    params(
        ("volume_id" = Uuid, Path, description = "Volume ID")
    ),
    request_body = CreateSnapshotRequest,
    responses(
        (status = 201, description = "Snapshot created", body = VolumeSnapshot),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Volume not found")
    ),
    tag = "volume",
    security(("jwt" = []))
)]
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

    Ok((StatusCode::CREATED, Json(snapshot)))
}

#[utoipa::path(
    delete,
    path = "/v1/volumes/{volume_id}",
    params(
        ("volume_id" = Uuid, Path, description = "Volume ID")
    ),
    responses(
        (status = 204, description = "Volume deleted"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Volume not found")
    ),
    tag = "volume",
    security(("jwt" = []))
)]
pub async fn delete_volume_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(volume_id): Path<Uuid>,
) -> ApiResult<axum::response::Response> {
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

    Ok(StatusCode::NO_CONTENT.into_response())
}

#[utoipa::path(
    post,
    path = "/v1/volumes/{volume_id}/restore",
    params(
        ("volume_id" = Uuid, Path, description = "Volume ID")
    ),
    request_body = RestoreSnapshotRequest,
    responses(
        (status = 200, description = "Snapshot restored successfully"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Volume not found")
    ),
    tag = "volume",
    security(("jwt" = []))
)]
pub async fn restore_snapshot_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(volume_id): Path<Uuid>,
    Json(req): Json<RestoreSnapshotRequest>,
) -> ApiResult<axum::response::Response> {
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

    Ok(StatusCode::OK.into_response())
}

#[utoipa::path(
    delete,
    path = "/v1/snapshots/{snapshot_id}",
    params(
        ("snapshot_id" = Uuid, Path, description = "Snapshot ID")
    ),
    responses(
        (status = 204, description = "Snapshot deleted"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Snapshot not found")
    ),
    tag = "volume",
    security(("jwt" = []))
)]
pub async fn delete_snapshot_handler(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(snapshot_id): Path<Uuid>,
) -> ApiResult<axum::response::Response> {
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

    Ok(StatusCode::NO_CONTENT.into_response())
}

#[utoipa::path(
    post,
    path = "/v1/volumes/{volume_id}/clone",
    params(
        ("volume_id" = Uuid, Path, description = "Source volume ID")
    ),
    request_body = CloneVolumeRequest,
    responses(
        (status = 201, description = "Volume cloned successfully", body = Volume),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Volume not found")
    ),
    tag = "volume",
    security(("jwt" = []))
)]
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

    Ok((StatusCode::CREATED, Json(target_volume)))
}
