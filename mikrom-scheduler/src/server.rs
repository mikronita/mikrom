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
            })
            .unwrap_or_default();

        let strategy = crate::domain::worker::SchedulingStrategy::LeastLoaded;

        match self
            .app_service
            .deployment
            .deploy_app(
                req.app_id.clone(),
                req.app_name,
                req.image,
                req.user_id,
                req.deployment_id,
                req.vpc_ipv6_prefix,
                config,
                strategy,
            )
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
