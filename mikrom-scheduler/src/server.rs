use crate::application::AppService;
use crate::domain::{VmConfig, Volume};
use mikrom_proto::scheduler::{
    AppInfo, AppStatusResponse, CancelRequest, CancelResponse, DeleteAllByAppRequest,
    DeleteAllByAppResponse, DeleteAppRequest, DeleteAppResponse, DeployRequest, DeployResponse,
    ListAppsRequest, ListAppsResponse, ListWorkersRequest, ListWorkersResponse, PauseRequest,
    PauseResponse, ResumeRequest, ResumeResponse, UpdateAppScalingConfigRequest,
    UpdateAppScalingConfigResponse, UpdateSecurityGroupsRequest, UpdateSecurityGroupsResponse,
};
use mikrom_proto::tls::ServiceCerts;
use std::sync::Arc;

pub struct SchedulerServer {
    pub app_service: Arc<AppService>,
    pub certs: Option<ServiceCerts>,
}

impl SchedulerServer {
    pub fn new(app_service: Arc<AppService>, certs: Option<ServiceCerts>) -> Self {
        Self { app_service, certs }
    }

    #[tracing::instrument(skip(self, req), fields(app_id = %req.app_id))]
    pub async fn deploy_app(&self, req: DeployRequest) -> anyhow::Result<DeployResponse> {
        let config = req
            .config
            .map(|c| VmConfig {
                vcpus: c.vcpus,
                memory_mib: u64::from(c.memory_mib),
                disk_mib: u64::from(c.disk_mib),
                port: c.port,
                env: c.env,
                ipv6_address: Some(c.ipv6_address),
                ipv6_gateway: Some(c.ipv6_gateway),
                volumes: c
                    .volumes
                    .iter()
                    .map(|v| Volume {
                        volume_id: v.volume_id.clone(),
                        size_mib: v.size_mib,
                        read_only: v.read_only,
                        pool_name: v.pool_name.clone(),
                        mount_point: v.mount_point.clone(),
                        access_mode: match v.access_mode {
                            1 => crate::domain::job::AccessMode::ReadWriteMany,
                            2 => crate::domain::job::AccessMode::ReadOnlyMany,
                            _ => crate::domain::job::AccessMode::ReadWriteOnce,
                        },
                    })
                    .collect(),
                health_check_path: c.health_check_path,
                hypervisor: crate::domain::job::HypervisorType::from_i32(c.hypervisor)
                    .unwrap_or_default(),
            })
            .unwrap_or_default();

        let strategy = crate::domain::worker::SchedulingStrategy::LeastLoaded;

        match self
            .app_service
            .deployment
            .deploy_app(crate::application::deployment::DeployAppParams {
                app_id: req.app_id.clone(),
                app_name: req.app_name,
                image: req.image,
                user_id: req.user_id,
                deployment_id: req.deployment_id,
                vpc_ipv6_prefix: req.vpc_ipv6_prefix,
                config,
                strategy,
            })
            .await
        {
            Ok(job) => Ok(DeployResponse {
                job_id: job.job_id,
                status: mikrom_proto::scheduler::DeployStatus::Running as i32,
                host_id: job.host_id.unwrap_or_default(),
                vm_id: job.vm_id.unwrap_or_default(),
                message: "Deployment successful".to_string(),
            }),
            Err(e) => {
                tracing::error!("Deployment failed for app {}: {}", req.app_id, e);
                Ok(DeployResponse {
                    status: mikrom_proto::scheduler::DeployStatus::Failed as i32,
                    message: format!("Deployment failed: {}", e),
                    ..Default::default()
                })
            },
        }
    }

    pub async fn get_app_status(
        &self,
        req: mikrom_proto::scheduler::AppStatusRequest,
    ) -> anyhow::Result<AppStatusResponse> {
        match self
            .app_service
            .get_app_status(&req.job_id, &req.user_id)
            .await
        {
            Ok(job) => {
                let (cpu_usage, ram_used_bytes, tx_bytes, rx_bytes) =
                    self.app_service.get_job_metrics(&job).await;
                Ok(AppStatusResponse {
                    job_id: job.job_id,
                    status: job.status as i32,
                    host_id: job.host_id.unwrap_or_default(),
                    vm_id: job.vm_id.unwrap_or_default(),
                    scheduled_at: job.scheduled_at.unwrap_or(0),
                    started_at: job.started_at.unwrap_or(0),
                    stopped_at: job.stopped_at.unwrap_or(0),
                    error_message: job.error_message.unwrap_or_default(),
                    cpu_usage,
                    ram_used_bytes,
                    ipv6_address: job.config.ipv6_address.unwrap_or_default(),
                    tx_bytes,
                    rx_bytes,
                })
            },
            Err(e) => Err(anyhow::anyhow!(e.to_string())),
        }
    }

    pub async fn list_apps(&self, req: ListAppsRequest) -> anyhow::Result<ListAppsResponse> {
        let status_filter = req.status.and_then(crate::domain::JobStatus::from_i32);

        let jobs = self
            .app_service
            .job_repo
            .list_jobs(Some(&req.user_id), None, status_filter)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        let mut apps = Vec::new();
        for job in jobs {
            let (cpu_usage, ram_used_bytes, tx_bytes, rx_bytes) =
                self.app_service.get_job_metrics(&job).await;
            apps.push(AppInfo {
                job_id: job.job_id,
                app_id: job.app_id,
                app_name: job.app_name,
                image: job.image,
                status: job.status as i32,
                host_id: job.host_id.unwrap_or_default(),
                vm_id: job.vm_id.unwrap_or_default(),
                cpu_usage,
                ram_used_bytes,
                user_id: job.user_id,
                deployment_id: job.deployment_id.unwrap_or_default(),
                ipv6_address: job.config.ipv6_address.unwrap_or_default(),
                tx_bytes,
                rx_bytes,
            });
        }
        Ok(ListAppsResponse { apps })
    }

    pub async fn pause_app(&self, req: PauseRequest) -> anyhow::Result<PauseResponse> {
        match self.app_service.pause_app(&req.job_id, &req.user_id).await {
            Ok(_) => Ok(PauseResponse {
                success: true,
                message: "Paused".to_string(),
            }),
            Err(e) => Ok(PauseResponse {
                success: false,
                message: e.to_string(),
            }),
        }
    }

    pub async fn resume_app(&self, req: ResumeRequest) -> anyhow::Result<ResumeResponse> {
        match self.app_service.resume_app(&req.job_id, &req.user_id).await {
            Ok(_) => Ok(ResumeResponse {
                success: true,
                message: "Resumed".to_string(),
            }),
            Err(e) => Ok(ResumeResponse {
                success: false,
                message: e.to_string(),
            }),
        }
    }

    pub async fn cancel_app(&self, req: CancelRequest) -> anyhow::Result<CancelResponse> {
        match self
            .app_service
            .job_repo
            .cancel_job(&req.job_id, chrono::Utc::now().timestamp())
            .await
        {
            Ok(_) => Ok(CancelResponse {
                success: true,
                message: "Cancelled".to_string(),
            }),
            Err(e) => Ok(CancelResponse {
                success: false,
                message: e.to_string(),
            }),
        }
    }

    pub async fn delete_app(&self, req: DeleteAppRequest) -> anyhow::Result<DeleteAppResponse> {
        match self.app_service.delete_app(&req.job_id, &req.user_id).await {
            Ok(_) => Ok(DeleteAppResponse {
                success: true,
                message: "Deleted".to_string(),
            }),
            Err(e) => Ok(DeleteAppResponse {
                success: false,
                message: e.to_string(),
            }),
        }
    }

    pub async fn delete_all_by_app(
        &self,
        req: DeleteAllByAppRequest,
    ) -> anyhow::Result<DeleteAllByAppResponse> {
        match self
            .app_service
            .delete_all_by_app(&req.app_id, &req.user_id)
            .await
        {
            Ok(_) => Ok(DeleteAllByAppResponse {
                success: true,
                message: "All app jobs deleted successfully".to_string(),
            }),
            Err(e) => Ok(DeleteAllByAppResponse {
                success: false,
                message: e.to_string(),
            }),
        }
    }

    pub async fn scale_app(
        &self,
        req: mikrom_proto::scheduler::ScaleAppRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::ScaleAppResponse> {
        match self
            .app_service
            .scale_app(&req.app_id, req.desired_replicas, &req.user_id)
            .await
        {
            Ok(_) => Ok(mikrom_proto::scheduler::ScaleAppResponse {
                success: true,
                message: format!("App scaled to {} replicas", req.desired_replicas),
            }),
            Err(e) => {
                tracing::error!(app_id = %req.app_id, error = %e, "Scale operation failed");
                Ok(mikrom_proto::scheduler::ScaleAppResponse {
                    success: false,
                    message: format!("Failed to scale app: {}", e),
                })
            },
        }
    }

    pub async fn update_app_scaling_config(
        &self,
        req: UpdateAppScalingConfigRequest,
    ) -> anyhow::Result<UpdateAppScalingConfigResponse> {
        match self
            .app_service
            .app_repo
            .update_app_config(crate::domain::AppConfig {
                id: req.app_id,
                user_id: req.user_id,
                vpc_ipv6_prefix: req.vpc_ipv6_prefix,
                hostname: req.hostname,
                desired_replicas: req.desired_replicas,
                min_replicas: req.min_replicas,
                max_replicas: req.max_replicas,
                autoscaling_enabled: req.autoscaling_enabled,
                cpu_threshold: req.cpu_threshold,
                mem_threshold: req.mem_threshold,
                last_router_traffic_at: req.last_router_traffic_at,
                last_scaled_to_zero_at: req.last_scaled_to_zero_at,
                restore_retry_after_at: 0,
            })
            .await
        {
            Ok(_) => Ok(UpdateAppScalingConfigResponse {
                success: true,
                message: "App scaling config updated".to_string(),
            }),
            Err(e) => Ok(UpdateAppScalingConfigResponse {
                success: false,
                message: e.to_string(),
            }),
        }
    }

    pub async fn list_workers(
        &self,
        _req: ListWorkersRequest,
    ) -> anyhow::Result<ListWorkersResponse> {
        let workers = self
            .app_service
            .worker_repo
            .list_workers()
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        let worker_infos = workers
            .into_iter()
            .map(|w| mikrom_proto::scheduler::WorkerInfo {
                host_id: w.host_id,
                hostname: w.hostname,
                last_heartbeat: w.last_heartbeat,
                wireguard_pubkey: w.wireguard_pubkey.unwrap_or_default(),
                advertise_address: w.advertise_address,
            })
            .collect();

        Ok(ListWorkersResponse {
            workers: worker_infos,
        })
    }

    pub async fn check_health(
        &self,
        req: mikrom_proto::scheduler::CheckHealthRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::CheckHealthResponse> {
        match self
            .app_service
            .check_health(&req.job_id, &req.user_id)
            .await
        {
            Ok(is_healthy) => Ok(mikrom_proto::scheduler::CheckHealthResponse {
                is_healthy,
                message: if is_healthy {
                    "Healthy".to_string()
                } else {
                    "Unhealthy".to_string()
                },
            }),
            Err(e) => Ok(mikrom_proto::scheduler::CheckHealthResponse {
                is_healthy: false,
                message: e.to_string(),
            }),
        }
    }

    pub async fn update_security_groups(
        &self,
        req: UpdateSecurityGroupsRequest,
    ) -> anyhow::Result<UpdateSecurityGroupsResponse> {
        match self.app_service.update_security_groups(req).await {
            Ok(_) => Ok(UpdateSecurityGroupsResponse {
                success: true,
                message: "Security groups updated".to_string(),
            }),
            Err(e) => Ok(UpdateSecurityGroupsResponse {
                success: false,
                message: e.to_string(),
            }),
        }
    }

    pub async fn create_volume(
        &self,
        req: mikrom_proto::scheduler::CreateVolumeRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::CreateVolumeResponse> {
        match self
            .app_service
            .create_volume(&req.host_id, &req.volume_id, req.size_mib, &req.pool_name)
            .await
        {
            Ok(_) => Ok(mikrom_proto::scheduler::CreateVolumeResponse {
                success: true,
                message: "Volume created successfully".to_string(),
            }),
            Err(e) => Ok(mikrom_proto::scheduler::CreateVolumeResponse {
                success: false,
                message: e.to_string(),
            }),
        }
    }

    pub async fn create_snapshot(
        &self,
        req: mikrom_proto::scheduler::CreateSnapshotRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::CreateSnapshotResponse> {
        match self
            .app_service
            .create_snapshot(
                &req.host_id,
                &req.volume_id,
                &req.snapshot_name,
                &req.pool_name,
            )
            .await
        {
            Ok(_) => Ok(mikrom_proto::scheduler::CreateSnapshotResponse {
                success: true,
                message: "Snapshot created successfully".to_string(),
            }),
            Err(e) => Ok(mikrom_proto::scheduler::CreateSnapshotResponse {
                success: false,
                message: format!("Failed to create snapshot: {}", e),
            }),
        }
    }

    pub async fn delete_volume(
        &self,
        req: mikrom_proto::scheduler::DeleteVolumeRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::DeleteVolumeResponse> {
        match self
            .app_service
            .delete_volume(&req.host_id, &req.volume_id, &req.pool_name)
            .await
        {
            Ok(_) => Ok(mikrom_proto::scheduler::DeleteVolumeResponse {
                success: true,
                message: "Volume deleted successfully".to_string(),
            }),
            Err(e) => Ok(mikrom_proto::scheduler::DeleteVolumeResponse {
                success: false,
                message: format!("Failed to delete volume: {}", e),
            }),
        }
    }

    pub async fn delete_snapshot(
        &self,
        req: mikrom_proto::scheduler::DeleteSnapshotRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::DeleteSnapshotResponse> {
        match self
            .app_service
            .delete_snapshot(
                &req.host_id,
                &req.volume_id,
                &req.snapshot_name,
                &req.pool_name,
            )
            .await
        {
            Ok(_) => Ok(mikrom_proto::scheduler::DeleteSnapshotResponse {
                success: true,
                message: "Snapshot deleted successfully".to_string(),
            }),
            Err(e) => Ok(mikrom_proto::scheduler::DeleteSnapshotResponse {
                success: false,
                message: format!("Failed to delete snapshot: {}", e),
            }),
        }
    }

    pub async fn restore_snapshot(
        &self,
        req: mikrom_proto::scheduler::RestoreSnapshotRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::RestoreSnapshotResponse> {
        match self
            .app_service
            .restore_snapshot(
                &req.host_id,
                &req.volume_id,
                &req.snapshot_name,
                &req.pool_name,
            )
            .await
        {
            Ok(_) => Ok(mikrom_proto::scheduler::RestoreSnapshotResponse {
                success: true,
                message: "Snapshot restored successfully".to_string(),
            }),
            Err(e) => Ok(mikrom_proto::scheduler::RestoreSnapshotResponse {
                success: false,
                message: format!("Failed to restore snapshot: {}", e),
            }),
        }
    }

    pub async fn clone_volume(
        &self,
        req: mikrom_proto::scheduler::CloneVolumeRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::CloneVolumeResponse> {
        match self
            .app_service
            .clone_volume(
                &req.host_id,
                &req.source_volume_id,
                &req.snapshot_name,
                &req.target_volume_id,
                &req.pool_name,
            )
            .await
        {
            Ok(_) => Ok(mikrom_proto::scheduler::CloneVolumeResponse {
                success: true,
                message: "Volume cloned successfully".to_string(),
            }),
            Err(e) => Ok(mikrom_proto::scheduler::CloneVolumeResponse {
                success: false,
                message: format!("Failed to clone volume: {}", e),
            }),
        }
    }

    pub async fn vm_snapshot_create(
        &self,
        req: mikrom_proto::scheduler::VmSnapshotCreateRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::VmSnapshotCreateResponse> {
        let job = match self
            .app_service
            .get_app_status(&req.job_id, &req.user_id)
            .await
        {
            Ok(j) => j,
            Err(e) => {
                return Ok(mikrom_proto::scheduler::VmSnapshotCreateResponse {
                    success: false,
                    message: e.to_string(),
                });
            },
        };
        let (host_id, vm_id) = match (&job.host_id, &job.vm_id) {
            (Some(h), Some(v)) => (h.clone(), v.clone()),
            _ => {
                return Ok(mikrom_proto::scheduler::VmSnapshotCreateResponse {
                    success: false,
                    message: "Job has no host or VM assigned".to_string(),
                });
            },
        };
        match self
            .app_service
            .agent_client
            .vm_snapshot_create(&host_id, &vm_id, &req.snapshot_name)
            .await
        {
            Ok(_) => Ok(mikrom_proto::scheduler::VmSnapshotCreateResponse {
                success: true,
                message: "VM snapshot created".to_string(),
            }),
            Err(e) => Ok(mikrom_proto::scheduler::VmSnapshotCreateResponse {
                success: false,
                message: e.to_string(),
            }),
        }
    }

    pub async fn vm_snapshot_restore(
        &self,
        req: mikrom_proto::scheduler::VmSnapshotRestoreRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::VmSnapshotRestoreResponse> {
        let job = match self
            .app_service
            .get_app_status(&req.job_id, &req.user_id)
            .await
        {
            Ok(j) => j,
            Err(e) => {
                return Ok(mikrom_proto::scheduler::VmSnapshotRestoreResponse {
                    success: false,
                    message: e.to_string(),
                });
            },
        };
        let (host_id, vm_id) = match (&job.host_id, &job.vm_id) {
            (Some(h), Some(v)) => (h.clone(), v.clone()),
            _ => {
                return Ok(mikrom_proto::scheduler::VmSnapshotRestoreResponse {
                    success: false,
                    message: "Job has no host or VM assigned".to_string(),
                });
            },
        };
        match self
            .app_service
            .agent_client
            .vm_snapshot_restore(&host_id, &vm_id, &req.snapshot_name)
            .await
        {
            Ok(_) => Ok(mikrom_proto::scheduler::VmSnapshotRestoreResponse {
                success: true,
                message: "VM snapshot restored".to_string(),
            }),
            Err(e) => Ok(mikrom_proto::scheduler::VmSnapshotRestoreResponse {
                success: false,
                message: e.to_string(),
            }),
        }
    }

    pub async fn vm_snapshot_delete(
        &self,
        req: mikrom_proto::scheduler::VmSnapshotDeleteRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::VmSnapshotDeleteResponse> {
        let job = match self
            .app_service
            .get_app_status(&req.job_id, &req.user_id)
            .await
        {
            Ok(j) => j,
            Err(e) => {
                return Ok(mikrom_proto::scheduler::VmSnapshotDeleteResponse {
                    success: false,
                    message: e.to_string(),
                });
            },
        };
        let (host_id, vm_id) = match (&job.host_id, &job.vm_id) {
            (Some(h), Some(v)) => (h.clone(), v.clone()),
            _ => {
                return Ok(mikrom_proto::scheduler::VmSnapshotDeleteResponse {
                    success: false,
                    message: "Job has no host or VM assigned".to_string(),
                });
            },
        };
        match self
            .app_service
            .agent_client
            .vm_snapshot_delete(&host_id, &vm_id, &req.snapshot_name)
            .await
        {
            Ok(_) => Ok(mikrom_proto::scheduler::VmSnapshotDeleteResponse {
                success: true,
                message: "VM snapshot deleted".to_string(),
            }),
            Err(e) => Ok(mikrom_proto::scheduler::VmSnapshotDeleteResponse {
                success: false,
                message: e.to_string(),
            }),
        }
    }

    pub async fn vm_snapshot_list(
        &self,
        req: mikrom_proto::scheduler::VmSnapshotListRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::VmSnapshotListResponse> {
        let job = match self
            .app_service
            .get_app_status(&req.job_id, &req.user_id)
            .await
        {
            Ok(j) => j,
            Err(e) => {
                return Ok(mikrom_proto::scheduler::VmSnapshotListResponse {
                    success: false,
                    message: e.to_string(),
                    snapshots: vec![],
                });
            },
        };
        let (host_id, vm_id) = match (&job.host_id, &job.vm_id) {
            (Some(h), Some(v)) => (h.clone(), v.clone()),
            _ => {
                return Ok(mikrom_proto::scheduler::VmSnapshotListResponse {
                    success: false,
                    message: "Job has no host or VM assigned".to_string(),
                    snapshots: vec![],
                });
            },
        };
        match self
            .app_service
            .agent_client
            .vm_snapshot_list(&host_id, &vm_id)
            .await
        {
            Ok(snaps) => {
                let snapshots: Vec<mikrom_proto::scheduler::VmSnapshotInfo> = snaps
                    .into_iter()
                    .map(|s| mikrom_proto::scheduler::VmSnapshotInfo {
                        id: s.id,
                        name: s.name,
                        created_at: s.created_at,
                        size_bytes: s.size_bytes,
                        vm_status: s.vm_status,
                    })
                    .collect();
                Ok(mikrom_proto::scheduler::VmSnapshotListResponse {
                    success: true,
                    message: "OK".to_string(),
                    snapshots,
                })
            },
            Err(e) => Ok(mikrom_proto::scheduler::VmSnapshotListResponse {
                success: false,
                message: e.to_string(),
                snapshots: vec![],
            }),
        }
    }

    pub async fn attach_volume(
        &self,
        req: mikrom_proto::scheduler::AttachVolumeRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::AttachVolumeResponse> {
        let job = match self
            .app_service
            .get_app_status(&req.job_id, &req.user_id)
            .await
        {
            Ok(j) => j,
            Err(e) => {
                return Ok(mikrom_proto::scheduler::AttachVolumeResponse {
                    success: false,
                    message: e.to_string(),
                });
            },
        };
        let (host_id, vm_id) = match (&job.host_id, &job.vm_id) {
            (Some(h), Some(v)) => (h.clone(), v.clone()),
            _ => {
                return Ok(mikrom_proto::scheduler::AttachVolumeResponse {
                    success: false,
                    message: "Job has no host or VM assigned".to_string(),
                });
            },
        };
        match self
            .app_service
            .agent_client
            .attach_volume(
                &host_id,
                &vm_id,
                &req.volume_id,
                &req.mount_point,
                req.read_only,
            )
            .await
        {
            Ok(_) => Ok(mikrom_proto::scheduler::AttachVolumeResponse {
                success: true,
                message: "Volume attached".to_string(),
            }),
            Err(e) => Ok(mikrom_proto::scheduler::AttachVolumeResponse {
                success: false,
                message: e.to_string(),
            }),
        }
    }

    pub async fn detach_volume(
        &self,
        req: mikrom_proto::scheduler::DetachVolumeRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::DetachVolumeResponse> {
        let job = match self
            .app_service
            .get_app_status(&req.job_id, &req.user_id)
            .await
        {
            Ok(j) => j,
            Err(e) => {
                return Ok(mikrom_proto::scheduler::DetachVolumeResponse {
                    success: false,
                    message: e.to_string(),
                });
            },
        };
        let (host_id, vm_id) = match (&job.host_id, &job.vm_id) {
            (Some(h), Some(v)) => (h.clone(), v.clone()),
            _ => {
                return Ok(mikrom_proto::scheduler::DetachVolumeResponse {
                    success: false,
                    message: "Job has no host or VM assigned".to_string(),
                });
            },
        };
        match self
            .app_service
            .agent_client
            .detach_volume(&host_id, &vm_id, &req.volume_id)
            .await
        {
            Ok(_) => Ok(mikrom_proto::scheduler::DetachVolumeResponse {
                success: true,
                message: "Volume detached".to_string(),
            }),
            Err(e) => Ok(mikrom_proto::scheduler::DetachVolumeResponse {
                success: false,
                message: e.to_string(),
            }),
        }
    }

    pub async fn start_migration(
        &self,
        req: mikrom_proto::scheduler::StartMigrationRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::StartMigrationResponse> {
        let job = match self
            .app_service
            .get_app_status(&req.job_id, &req.user_id)
            .await
        {
            Ok(j) => j,
            Err(e) => {
                return Ok(mikrom_proto::scheduler::StartMigrationResponse {
                    success: false,
                    message: e.to_string(),
                });
            },
        };
        let (host_id, vm_id) = match (&job.host_id, &job.vm_id) {
            (Some(h), Some(v)) => (h.clone(), v.clone()),
            _ => {
                return Ok(mikrom_proto::scheduler::StartMigrationResponse {
                    success: false,
                    message: "Job has no host or VM assigned".to_string(),
                });
            },
        };
        match self
            .app_service
            .agent_client
            .start_migration(&host_id, &vm_id, &req.target_host, &req.target_uri)
            .await
        {
            Ok(_) => Ok(mikrom_proto::scheduler::StartMigrationResponse {
                success: true,
                message: "Migration started".to_string(),
            }),
            Err(e) => Ok(mikrom_proto::scheduler::StartMigrationResponse {
                success: false,
                message: e.to_string(),
            }),
        }
    }

    pub async fn cancel_migration(
        &self,
        req: mikrom_proto::scheduler::CancelMigrationRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::CancelMigrationResponse> {
        let job = match self
            .app_service
            .get_app_status(&req.job_id, &req.user_id)
            .await
        {
            Ok(j) => j,
            Err(e) => {
                return Ok(mikrom_proto::scheduler::CancelMigrationResponse {
                    success: false,
                    message: e.to_string(),
                });
            },
        };
        let (host_id, vm_id) = match (&job.host_id, &job.vm_id) {
            (Some(h), Some(v)) => (h.clone(), v.clone()),
            _ => {
                return Ok(mikrom_proto::scheduler::CancelMigrationResponse {
                    success: false,
                    message: "Job has no host or VM assigned".to_string(),
                });
            },
        };
        match self
            .app_service
            .agent_client
            .cancel_migration(&host_id, &vm_id)
            .await
        {
            Ok(_) => Ok(mikrom_proto::scheduler::CancelMigrationResponse {
                success: true,
                message: "Migration cancelled".to_string(),
            }),
            Err(e) => Ok(mikrom_proto::scheduler::CancelMigrationResponse {
                success: false,
                message: e.to_string(),
            }),
        }
    }

    pub async fn query_migration(
        &self,
        req: mikrom_proto::scheduler::QueryMigrationRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::QueryMigrationResponse> {
        let job = match self
            .app_service
            .get_app_status(&req.job_id, &req.user_id)
            .await
        {
            Ok(j) => j,
            Err(e) => {
                return Ok(mikrom_proto::scheduler::QueryMigrationResponse {
                    success: false,
                    message: e.to_string(),
                    status: "".to_string(),
                    total_bytes: 0,
                    transferred_bytes: 0,
                    remaining_bytes: 0,
                });
            },
        };
        let (host_id, vm_id) = match (&job.host_id, &job.vm_id) {
            (Some(h), Some(v)) => (h.clone(), v.clone()),
            _ => {
                return Ok(mikrom_proto::scheduler::QueryMigrationResponse {
                    success: false,
                    message: "Job has no host or VM assigned".to_string(),
                    status: "".to_string(),
                    total_bytes: 0,
                    transferred_bytes: 0,
                    remaining_bytes: 0,
                });
            },
        };
        match self
            .app_service
            .agent_client
            .query_migration(&host_id, &vm_id)
            .await
        {
            Ok(status) => Ok(mikrom_proto::scheduler::QueryMigrationResponse {
                success: true,
                message: "OK".to_string(),
                status,
                total_bytes: 0,
                transferred_bytes: 0,
                remaining_bytes: 0,
            }),
            Err(e) => Ok(mikrom_proto::scheduler::QueryMigrationResponse {
                success: false,
                message: e.to_string(),
                status: "".to_string(),
                total_bytes: 0,
                transferred_bytes: 0,
                remaining_bytes: 0,
            }),
        }
    }

    pub async fn set_balloon(
        &self,
        req: mikrom_proto::scheduler::SetBalloonRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::SetBalloonResponse> {
        let job = match self
            .app_service
            .get_app_status(&req.job_id, &req.user_id)
            .await
        {
            Ok(j) => j,
            Err(e) => {
                return Ok(mikrom_proto::scheduler::SetBalloonResponse {
                    success: false,
                    message: e.to_string(),
                });
            },
        };
        let (host_id, vm_id) = match (&job.host_id, &job.vm_id) {
            (Some(h), Some(v)) => (h.clone(), v.clone()),
            _ => {
                return Ok(mikrom_proto::scheduler::SetBalloonResponse {
                    success: false,
                    message: "Job has no host or VM assigned".to_string(),
                });
            },
        };
        match self
            .app_service
            .agent_client
            .set_balloon(&host_id, &vm_id, req.target_memory_mib)
            .await
        {
            Ok(_) => Ok(mikrom_proto::scheduler::SetBalloonResponse {
                success: true,
                message: "Balloon size set".to_string(),
            }),
            Err(e) => Ok(mikrom_proto::scheduler::SetBalloonResponse {
                success: false,
                message: e.to_string(),
            }),
        }
    }

    pub async fn query_balloon(
        &self,
        req: mikrom_proto::scheduler::QueryBalloonRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::QueryBalloonResponse> {
        let job = match self
            .app_service
            .get_app_status(&req.job_id, &req.user_id)
            .await
        {
            Ok(j) => j,
            Err(e) => {
                return Ok(mikrom_proto::scheduler::QueryBalloonResponse {
                    success: false,
                    message: e.to_string(),
                    actual_memory_mib: 0,
                    max_memory_mib: 0,
                });
            },
        };
        let (host_id, vm_id) = match (&job.host_id, &job.vm_id) {
            (Some(h), Some(v)) => (h.clone(), v.clone()),
            _ => {
                return Ok(mikrom_proto::scheduler::QueryBalloonResponse {
                    success: false,
                    message: "Job has no host or VM assigned".to_string(),
                    actual_memory_mib: 0,
                    max_memory_mib: 0,
                });
            },
        };
        match self
            .app_service
            .agent_client
            .query_balloon(&host_id, &vm_id)
            .await
        {
            Ok((actual, max)) => Ok(mikrom_proto::scheduler::QueryBalloonResponse {
                success: true,
                message: "OK".to_string(),
                actual_memory_mib: actual,
                max_memory_mib: max,
            }),
            Err(e) => Ok(mikrom_proto::scheduler::QueryBalloonResponse {
                success: false,
                message: e.to_string(),
                actual_memory_mib: 0,
                max_memory_mib: 0,
            }),
        }
    }
}

impl Clone for SchedulerServer {
    fn clone(&self) -> Self {
        Self {
            app_service: self.app_service.clone(),
            certs: self.certs.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::AppService;
    use crate::domain::AppConfig;
    use crate::domain::worker::{MockAgentClient, MockJobRepository, MockWorkerRepository};
    use mockall::predicate::function;
    use std::sync::Arc;

    async fn connect_nats_or_skip() -> Option<async_nats::Client> {
        match async_nats::connect("nats://localhost:4223").await {
            Ok(client) => Some(client),
            Err(err) => {
                eprintln!("Skipping scheduler server test: failed to connect to NATS: {err}");
                None
            },
        }
    }

    #[tokio::test]
    async fn test_update_app_scaling_config_maps_router_activity_fields() {
        let mut app_repo = crate::domain::app::MockAppRepository::new();
        app_repo
            .expect_update_app_config()
            .with(function(|cfg: &AppConfig| {
                cfg.id == "app-1"
                    && cfg.user_id == "user-1"
                    && cfg.hostname == "app.example.com"
                    && cfg.last_router_traffic_at == 123
                    && cfg.last_scaled_to_zero_at == 456
                    && cfg.desired_replicas == 2
            }))
            .times(1)
            .returning(|_| Box::pin(async { Ok(()) }));

        let Some(nats_client) = connect_nats_or_skip().await else {
            return;
        };
        let service = AppService::new(
            Arc::new(MockJobRepository::new()),
            Arc::new(app_repo),
            Arc::new(MockWorkerRepository::new()),
            Arc::new(MockAgentClient::new()),
            nats_client,
            sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap(),
            900,
        );
        let server = SchedulerServer::new(Arc::new(service), None);

        let response = server
            .update_app_scaling_config(UpdateAppScalingConfigRequest {
                app_id: "app-1".to_string(),
                user_id: "user-1".to_string(),
                vpc_ipv6_prefix: "fd00::".to_string(),
                hostname: "app.example.com".to_string(),
                desired_replicas: 2,
                min_replicas: 1,
                max_replicas: 3,
                autoscaling_enabled: true,
                cpu_threshold: 80.0,
                mem_threshold: 70.0,
                last_router_traffic_at: 123,
                last_scaled_to_zero_at: 456,
            })
            .await
            .unwrap();

        assert!(response.success);
    }
}
