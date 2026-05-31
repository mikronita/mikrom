use crate::AppState;
use crate::domain::App;
use crate::domain::Deployment;
pub use crate::domain::worker::MeshStatus;
use crate::error::{ApiError, ApiResult};
use serde::Serialize;
use tokio_stream::StreamExt;
use uuid::Uuid;

pub struct VmService;

#[derive(Debug, Clone, Serialize, rovo::schemars::JsonSchema)]
pub struct VmSnapshot {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub size_bytes: u64,
    pub vm_status: String,
}

pub struct LiveDeploymentInfoParams<'a> {
    pub app_id: String,
    pub app_name: String,
    pub deployment: Option<&'a Deployment>,
    pub job_id: String,
    pub host_id: String,
    pub vm_id: String,
    pub image: String,
    pub status: String,
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub ipv6_address: Option<String>,
    pub hypervisor: Option<String>,
    pub scale_state: crate::application::deployment::AppScaleState,
}

pub struct LiveDeploymentStatusParams<'a> {
    pub app_id: String,
    pub app_name: String,
    pub deployment: &'a Deployment,
    pub job_id: String,
    pub status: String,
    pub host_id: String,
    pub vm_id: String,
    pub scheduled_at: i64,
    pub started_at: i64,
    pub stopped_at: i64,
    pub error_message: String,
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub ipv6_address: Option<String>,
    pub hypervisor: Option<String>,
    pub scale_state: crate::application::deployment::AppScaleState,
}

pub struct LiveDeploymentEventParams<'a> {
    pub app_id: String,
    pub app_name: String,
    pub deployment: Option<&'a Deployment>,
    pub job_id: String,
    pub image: String,
    pub status: String,
    pub host_id: String,
    pub vm_id: String,
    pub ipv6_address: Option<String>,
    pub hypervisor: Option<String>,
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub scheduled_at: i64,
    pub started_at: i64,
    pub stopped_at: i64,
    pub error_message: String,
    pub scale_state: crate::application::deployment::AppScaleState,
}

#[derive(Debug, Clone, Serialize, rovo::schemars::JsonSchema)]
pub struct LiveDeploymentInfo {
    pub job_id: String,
    pub deployment_id: String,
    pub app_id: String,
    pub app_name: String,
    pub image: String,
    pub status: String,
    pub host_id: String,
    pub vm_id: String,
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub ipv6_address: Option<String>,
    pub hypervisor: Option<String>,
    pub vcpus: i32,
    pub memory_mib: i64,
    pub scale_state: crate::application::deployment::AppScaleState,
}

#[derive(Debug, Clone, Serialize, rovo::schemars::JsonSchema)]
pub struct LiveDeploymentStatus {
    pub job_id: String,
    pub deployment_id: String,
    pub app_id: String,
    pub app_name: String,
    pub image: String,
    pub status: String,
    pub host_id: String,
    pub vm_id: String,
    pub scheduled_at: i64,
    pub started_at: i64,
    pub stopped_at: i64,
    pub error_message: String,
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub ipv6_address: Option<String>,
    pub hypervisor: Option<String>,
    pub vcpus: i32,
    pub memory_mib: i64,
    pub scale_state: crate::application::deployment::AppScaleState,
}

#[derive(Debug, Clone, Serialize, rovo::schemars::JsonSchema)]
pub struct LiveDeploymentEvent {
    pub job_id: String,
    pub deployment_id: String,
    pub app_id: String,
    pub app_name: String,
    pub image: String,
    pub status: String,
    pub git_commit_hash: Option<String>,
    pub git_commit_message: Option<String>,
    pub git_branch: Option<String>,
    pub host_id: String,
    pub vm_id: String,
    pub ipv6_address: Option<String>,
    pub hypervisor: Option<String>,
    pub vcpus: i32,
    pub memory_mib: i64,
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub scale_state: crate::application::deployment::AppScaleState,
    pub scheduled_at: i64,
    pub started_at: i64,
    pub stopped_at: i64,
    pub error_message: String,
}

#[derive(Debug, Clone, Serialize, rovo::schemars::JsonSchema)]
pub struct OperationResult {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, rovo::schemars::JsonSchema)]
pub struct SnapshotListResult {
    pub success: bool,
    pub message: String,
    pub snapshots: Vec<crate::application::vms::VmSnapshot>,
}

#[derive(Debug, Clone, Serialize, rovo::schemars::JsonSchema)]
pub struct BalloonQueryResult {
    pub success: bool,
    pub message: String,
    pub actual_memory_mib: u32,
    pub max_memory_mib: u32,
}

impl VmService {
    pub async fn validate_app_deployment(
        state: &AppState,
        tenant_id: &str,
        app_name: &str,
        job_id: &str,
    ) -> ApiResult<(App, Deployment)> {
        let app = state
            .app_repo
            .get_app_by_name(app_name)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or(ApiError::NotFound("Application not found".into()))?;

        if app.tenant_id.to_string() != tenant_id {
            return Err(ApiError::Forbidden);
        }

        let deployment = if let Some(stripped) = job_id.strip_prefix("temp-") {
            let dep_id = Uuid::parse_str(stripped)
                .map_err(|_| ApiError::BadRequest("Invalid temp ID".into()))?;
            state
                .app_repo
                .get_deployment(dep_id)
                .await
                .map_err(|e| ApiError::Internal(e.to_string()))?
                .ok_or(ApiError::NotFound("Deployment not found".into()))?
        } else {
            state
                .app_repo
                .get_deployment_by_job_id(job_id)
                .await
                .map_err(|e| ApiError::Internal(e.to_string()))?
                .ok_or(ApiError::NotFound("Deployment not found".into()))?
        };

        if deployment.app_id != app.id {
            return Err(ApiError::BadRequest(
                "Deployment does not belong to this application".into(),
            ));
        }

        Ok((app, deployment))
    }

    pub async fn build_live_deployment_info(
        params: LiveDeploymentInfoParams<'_>,
    ) -> LiveDeploymentInfo {
        let vcpus = params
            .deployment
            .map(|deployment| deployment.vcpus.value() as i32)
            .unwrap_or(1);
        let memory_mib = params
            .deployment
            .map(|deployment| deployment.memory_mib.value() as i64)
            .unwrap_or(128);
        let ipv6_address = params
            .deployment
            .and_then(|deployment| deployment.ipv6_address.clone());

        LiveDeploymentInfo {
            job_id: params.job_id,
            deployment_id: params
                .deployment
                .map(|deployment| deployment.id.to_string())
                .unwrap_or_default(),
            app_id: params.app_id,
            app_name: params.app_name,
            image: params.image,
            status: params.status,
            host_id: params.host_id,
            vm_id: params.vm_id,
            cpu_usage: params.cpu_usage,
            ram_used_bytes: params.ram_used_bytes,
            tx_bytes: params.tx_bytes,
            rx_bytes: params.rx_bytes,
            ipv6_address,
            hypervisor: params.hypervisor,
            vcpus,
            memory_mib,
            scale_state: params.scale_state,
        }
    }

    pub fn build_live_deployment_status(
        params: LiveDeploymentStatusParams<'_>,
    ) -> LiveDeploymentStatus {
        LiveDeploymentStatus {
            job_id: params.job_id,
            deployment_id: params.deployment.id.to_string(),
            app_id: params.app_id,
            app_name: params.app_name,
            image: params.deployment.image_tag.clone().unwrap_or_default(),
            status: params.status,
            host_id: params.host_id,
            vm_id: params.vm_id,
            scheduled_at: params.scheduled_at,
            started_at: params.started_at,
            stopped_at: params.stopped_at,
            error_message: params.error_message,
            cpu_usage: params.cpu_usage,
            ram_used_bytes: params.ram_used_bytes,
            tx_bytes: params.tx_bytes,
            rx_bytes: params.rx_bytes,
            ipv6_address: params.ipv6_address,
            hypervisor: params.hypervisor,
            vcpus: params.deployment.vcpus.value() as i32,
            memory_mib: params.deployment.memory_mib.value() as i64,
            scale_state: params.scale_state,
        }
    }

    pub fn build_live_deployment_event(
        params: LiveDeploymentEventParams<'_>,
    ) -> LiveDeploymentEvent {
        let vcpus = params
            .deployment
            .map(|deployment| deployment.vcpus.value() as i32)
            .unwrap_or(1);
        let memory_mib = params
            .deployment
            .map(|deployment| deployment.memory_mib.value() as i64)
            .unwrap_or(128);
        let git_commit_hash = params
            .deployment
            .and_then(|deployment| deployment.git_commit_hash.clone());
        let git_commit_message = params
            .deployment
            .and_then(|deployment| deployment.git_commit_message.clone());
        let git_branch = params
            .deployment
            .and_then(|deployment| deployment.git_branch.clone());

        LiveDeploymentEvent {
            job_id: params.job_id,
            deployment_id: params
                .deployment
                .map(|deployment| deployment.id.to_string())
                .unwrap_or_default(),
            app_id: params.app_id,
            app_name: params.app_name,
            image: params.image,
            status: params.status,
            git_commit_hash,
            git_commit_message,
            git_branch,
            host_id: params.host_id,
            vm_id: params.vm_id,
            ipv6_address: params.ipv6_address,
            hypervisor: params.hypervisor,
            vcpus,
            memory_mib,
            cpu_usage: params.cpu_usage,
            ram_used_bytes: params.ram_used_bytes,
            tx_bytes: params.tx_bytes,
            rx_bytes: params.rx_bytes,
            scale_state: params.scale_state,
            scheduled_at: params.scheduled_at,
            started_at: params.started_at,
            stopped_at: params.stopped_at,
            error_message: params.error_message,
        }
    }

    pub async fn create_snapshot(
        state: &AppState,
        tenant_id: String,
        job_id: String,
        snapshot_name: String,
    ) -> ApiResult<(bool, String)> {
        let nats_req = mikrom_proto::scheduler::VmSnapshotCreateRequest {
            job_id,
            tenant_id,
            snapshot_name,
        };
        let resp: mikrom_proto::scheduler::VmSnapshotCreateResponse = state
            .nats
            .request("mikrom.scheduler.vm_snapshot_create", nats_req)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        Ok((resp.success, resp.message))
    }

    pub async fn restore_snapshot(
        state: &AppState,
        tenant_id: String,
        job_id: String,
        snapshot_name: String,
    ) -> ApiResult<(bool, String)> {
        let nats_req = mikrom_proto::scheduler::VmSnapshotRestoreRequest {
            job_id,
            tenant_id,
            snapshot_name,
        };
        let resp: mikrom_proto::scheduler::VmSnapshotRestoreResponse = state
            .nats
            .request("mikrom.scheduler.vm_snapshot_restore", nats_req)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        Ok((resp.success, resp.message))
    }

    pub async fn delete_snapshot(
        state: &AppState,
        tenant_id: String,
        job_id: String,
        snapshot_name: String,
    ) -> ApiResult<(bool, String)> {
        let nats_req = mikrom_proto::scheduler::VmSnapshotDeleteRequest {
            job_id,
            tenant_id,
            snapshot_name,
        };
        let resp: mikrom_proto::scheduler::VmSnapshotDeleteResponse = state
            .nats
            .request("mikrom.scheduler.vm_snapshot_delete", nats_req)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        Ok((resp.success, resp.message))
    }

    pub async fn list_snapshots(
        state: &AppState,
        tenant_id: String,
        job_id: String,
    ) -> ApiResult<(bool, String, Vec<VmSnapshot>)> {
        let nats_req = mikrom_proto::scheduler::VmSnapshotListRequest { job_id, tenant_id };
        let resp: mikrom_proto::scheduler::VmSnapshotListResponse = state
            .nats
            .request("mikrom.scheduler.vm_snapshot_list", nats_req)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        let snapshots = resp
            .snapshots
            .into_iter()
            .map(|s| VmSnapshot {
                id: s.id,
                name: s.name,
                created_at: s.created_at,
                size_bytes: s.size_bytes as u64,
                vm_status: s.vm_status,
            })
            .collect();

        Ok((resp.success, resp.message, snapshots))
    }

    pub async fn attach_volume(
        state: &AppState,
        tenant_id: String,
        job_id: String,
        volume_id: String,
        mount_point: String,
        read_only: bool,
    ) -> ApiResult<(bool, String)> {
        let nats_req = mikrom_proto::scheduler::AttachVolumeRequest {
            job_id,
            tenant_id,
            volume_id,
            mount_point,
            read_only,
        };
        let resp: mikrom_proto::scheduler::AttachVolumeResponse = state
            .nats
            .request("mikrom.scheduler.attach_volume", nats_req)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        Ok((resp.success, resp.message))
    }

    pub async fn detach_volume(
        state: &AppState,
        tenant_id: String,
        job_id: String,
        volume_id: String,
    ) -> ApiResult<(bool, String)> {
        let nats_req = mikrom_proto::scheduler::DetachVolumeRequest {
            job_id,
            tenant_id,
            volume_id,
        };
        let resp: mikrom_proto::scheduler::DetachVolumeResponse = state
            .nats
            .request("mikrom.scheduler.detach_volume", nats_req)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        Ok((resp.success, resp.message))
    }

    pub async fn start_migration(
        state: &AppState,
        tenant_id: String,
        job_id: String,
        target_host: String,
        target_uri: String,
    ) -> ApiResult<(bool, String)> {
        let nats_req = mikrom_proto::scheduler::StartMigrationRequest {
            job_id,
            tenant_id,
            target_host,
            target_uri,
        };
        let resp: mikrom_proto::scheduler::StartMigrationResponse = state
            .nats
            .request("mikrom.scheduler.start_migration", nats_req)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        Ok((resp.success, resp.message))
    }

    pub async fn cancel_migration(
        state: &AppState,
        tenant_id: String,
        job_id: String,
    ) -> ApiResult<(bool, String)> {
        let nats_req = mikrom_proto::scheduler::CancelMigrationRequest { job_id, tenant_id };
        let resp: mikrom_proto::scheduler::CancelMigrationResponse = state
            .nats
            .request("mikrom.scheduler.cancel_migration", nats_req)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        Ok((resp.success, resp.message))
    }

    pub async fn query_migration(
        state: &AppState,
        tenant_id: String,
        job_id: String,
    ) -> ApiResult<(bool, String, String)> {
        let nats_req = mikrom_proto::scheduler::QueryMigrationRequest { job_id, tenant_id };
        let resp: mikrom_proto::scheduler::QueryMigrationResponse = state
            .nats
            .request("mikrom.scheduler.query_migration", nats_req)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        Ok((resp.success, resp.message, resp.status))
    }

    pub async fn set_balloon(
        state: &AppState,
        tenant_id: String,
        job_id: String,
        target_memory_mib: u32,
    ) -> ApiResult<(bool, String)> {
        let nats_req = mikrom_proto::scheduler::SetBalloonRequest {
            job_id,
            tenant_id,
            target_memory_mib,
        };
        let resp: mikrom_proto::scheduler::SetBalloonResponse = state
            .nats
            .request("mikrom.scheduler.set_balloon", nats_req)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        Ok((resp.success, resp.message))
    }

    pub async fn query_balloon(
        state: &AppState,
        tenant_id: String,
        job_id: String,
    ) -> ApiResult<(bool, String, u32, u32)> {
        let nats_req = mikrom_proto::scheduler::QueryBalloonRequest { job_id, tenant_id };
        let resp: mikrom_proto::scheduler::QueryBalloonResponse = state
            .nats
            .request("mikrom.scheduler.query_balloon", nats_req)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        Ok((
            resp.success,
            resp.message,
            resp.actual_memory_mib,
            resp.max_memory_mib,
        ))
    }
}

pub async fn fetch_mesh_status(state: &AppState) -> ApiResult<MeshStatus> {
    use crate::domain::Worker;

    let workers = state.scheduler.list_workers().await?;

    Ok(MeshStatus {
        total_workers: workers.workers.len(),
        workers: workers.workers.into_iter().map(Worker::from).collect(),
    })
}

pub async fn prime_mesh_status_cache(state: &AppState) -> ApiResult<()> {
    match fetch_mesh_status(state).await {
        Ok(snapshot) => {
            let _ = state.mesh_status.send(snapshot);
        },
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Failed to prime mesh status cache during startup; will be updated in background"
            );
        },
    }
    Ok(())
}

pub async fn refresh_mesh_status_cache(state: &AppState) -> ApiResult<MeshStatus> {
    let snapshot = fetch_mesh_status(state).await?;
    let _ = state.mesh_status.send(snapshot.clone());
    Ok(snapshot)
}

pub async fn start_mesh_status_tracker(state: crate::AppState) {
    let mut backoff = std::time::Duration::from_secs(1);
    let mut worker_heartbeat_sub = loop {
        match state
            .nats
            .subscribe("mikrom.scheduler.worker.heartbeat")
            .await
        {
            Ok(sub) => break sub,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    retry_after_secs = backoff.as_secs(),
                    "Failed to subscribe to worker heartbeats; retrying"
                );
                tokio::time::sleep(backoff).await;
                backoff = std::cmp::min(backoff * 2, std::time::Duration::from_secs(30));
            },
        }
    };

    backoff = std::time::Duration::from_secs(1);
    let mut router_heartbeat_sub = loop {
        match state
            .nats
            .subscribe("mikrom.scheduler.router.heartbeat")
            .await
        {
            Ok(sub) => break sub,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    retry_after_secs = backoff.as_secs(),
                    "Failed to subscribe to router heartbeats; retrying"
                );
                tokio::time::sleep(backoff).await;
                backoff = std::cmp::min(backoff * 2, std::time::Duration::from_secs(30));
            },
        }
    };
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));

    loop {
        tokio::select! {
            Some(_) = worker_heartbeat_sub.next() => {
                if let Err(err) = refresh_mesh_status_cache(&state).await {
                    tracing::warn!("failed to refresh mesh status after worker heartbeat: {}", err);
                }
            },
            Some(_) = router_heartbeat_sub.next() => {
                if let Err(err) = refresh_mesh_status_cache(&state).await {
                    tracing::warn!("failed to refresh mesh status after router heartbeat: {}", err);
                }
            },
            _ = interval.tick() => {
                if let Err(err) = refresh_mesh_status_cache(&state).await {
                    tracing::warn!("failed to refresh mesh status on interval: {}", err);
                }
            },
            else => break,
        }
    }
}
