use crate::application::AppService;
use crate::domain::{VmConfig, Volume};
use mikrom_proto::scheduler::{
    AppInfo, AppStatusResponse, CancelRequest, CancelResponse, DeleteAppRequest, DeleteAppResponse,
    DeployRequest, DeployResponse, ListAppsRequest, ListAppsResponse, ListWorkersRequest,
    ListWorkersResponse, PauseRequest, PauseResponse, ResumeRequest, ResumeResponse,
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
                ip_address: None,
                gateway: None,
                mac_address: None,
                netmask: None,
                volumes: c
                    .volumes
                    .iter()
                    .map(|v| Volume {
                        volume_id: v.volume_id.clone(),
                        size_mib: v.size_mib,
                        read_only: v.read_only,
                    })
                    .collect(),
            })
            .unwrap_or_default();

        let strategy = crate::domain::worker::SchedulingStrategy::LeastLoaded;

        match self
            .app_service
            .deployment
            .deploy_app(
                req.app_id,
                req.app_name,
                req.image,
                req.user_id,
                req.deployment_id,
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
                ip_address: job.config.ip_address.unwrap_or_default(),
            }),
            Err(e) => Ok(DeployResponse {
                message: format!("Deployment failed: {}", e),
                ..Default::default()
            }),
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
                let (cpu_usage, ram_used_bytes) = self.app_service.get_job_metrics(&job).await;
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
                    ip_address: job.config.ip_address.unwrap_or_default(),
                })
            },
            Err(e) => Err(anyhow::anyhow!(e.to_string())),
        }
    }

    pub async fn list_apps(&self, req: ListAppsRequest) -> anyhow::Result<ListAppsResponse> {
        let jobs = self
            .app_service
            .job_repo
            .list_jobs(Some(&req.user_id), None)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        let mut apps = Vec::new();
        for job in jobs {
            let (cpu_usage, ram_used_bytes) = self.app_service.get_job_metrics(&job).await;
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
                ip_address: w.ip_address,
                bridge_ip: w.bridge_ip,
                last_heartbeat: w.last_heartbeat,
            })
            .collect();

        Ok(ListWorkersResponse {
            workers: worker_infos,
        })
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
