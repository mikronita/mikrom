use crate::application::AppContext;
use crate::domain::{DomainError, DomainResult, Job, JobStatus};
use mikrom_proto::scheduler::{AppInfo, WorkerInfo};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppQueryService {
    ctx: Arc<AppContext>,
}

impl AppQueryService {
    pub fn new(ctx: Arc<AppContext>) -> Self {
        Self { ctx }
    }

    pub async fn get_app_status(&self, job_id: &str, user_id: &str) -> DomainResult<Job> {
        let telemetry = self.ctx.telemetry.clone();
        telemetry
            .observe_result("query", "get_app_status", async {
                let job = self
                    .ctx
                    .job_repo
                    .get_job(job_id)
                    .await?
                    .ok_or_else(|| DomainError::JobNotFound(job_id.to_string()))?;

                if job.user_id.as_ref() != user_id && user_id != "system" {
                    return Err(DomainError::Unauthorized(
                        "You do not own this job".to_string(),
                    ));
                }

                Ok(job)
            })
            .await
    }

    pub async fn check_health(&self, job_id: &str, user_id: &str) -> DomainResult<bool> {
        let telemetry = self.ctx.telemetry.clone();
        telemetry
            .observe_result("query", "check_health", async {
                let job = self.get_app_status(job_id, user_id).await?;
                if let (Some(host_id), Some(vm_id)) = (&job.host_id, &job.vm_id) {
                    self.ctx.agent_client.check_health(host_id, vm_id).await
                } else {
                    Ok(false)
                }
            })
            .await
    }

    pub async fn get_job_metrics(&self, job: &Job) -> (f32, u64, u64, u64) {
        let telemetry = self.ctx.telemetry.clone();
        telemetry
            .observe_value("query", "get_job_metrics", async {
                let metrics = async {
                    let host_id = job.host_id.as_ref()?;
                    let worker = self.ctx.worker_repo.get_worker(host_id).await.ok()??;
                    let metrics = worker.metrics.as_ref()?;
                    let vm_id = job.vm_id.as_ref()?;
                    metrics
                        .vms
                        .get(vm_id.as_ref())
                        .map(|m| (m.cpu_usage, m.ram_used_bytes, m.tx_bytes, m.rx_bytes))
                }
                .await;

                metrics.unwrap_or((0.0, 0, 0, 0))
            })
            .await
    }

    pub async fn resolve_hypervisor(&self, job: &Job) -> crate::domain::job::HypervisorType {
        if job.config.hypervisor != crate::domain::job::HypervisorType::Unspecified {
            return job.config.hypervisor;
        }
        if let Some(ref host_id) = job.host_id
            && let Ok(Some(worker)) = self.ctx.worker_repo.get_worker(host_id.as_ref()).await
            && !worker.supported_hypervisors.is_empty()
        {
            return worker.supported_hypervisors[0];
        }
        crate::domain::job::HypervisorType::Firecracker
    }

    pub async fn list_apps(
        &self,
        user_id: &str,
        status: Option<JobStatus>,
    ) -> DomainResult<Vec<AppInfo>> {
        let telemetry = self.ctx.telemetry.clone();
        telemetry
            .observe_result("query", "list_apps", async {
                let jobs = self
                    .ctx
                    .job_repo
                    .list_jobs(Some(user_id), None, status)
                    .await?;

                let workers = self.ctx.worker_repo.list_workers().await?;
                let worker_map: std::collections::HashMap<String, crate::domain::Worker> = workers
                    .into_iter()
                    .map(|w| (w.host_id.to_string(), w))
                    .collect();

                let mut apps = Vec::new();
                for job in jobs {
                    let (cpu_usage, ram_used_bytes, tx_bytes, rx_bytes) =
                        self.get_job_metrics(&job).await;
                    let hypervisor = if job.config.hypervisor
                        != crate::domain::job::HypervisorType::Unspecified
                    {
                        job.config.hypervisor
                    } else if let Some(ref host_id) = job.host_id {
                        worker_map
                            .get(host_id.as_ref())
                            .and_then(|w| w.supported_hypervisors.first().copied())
                            .unwrap_or(crate::domain::job::HypervisorType::Firecracker)
                    } else {
                        crate::domain::job::HypervisorType::Firecracker
                    };
                    apps.push(AppInfo {
                        job_id: job.job_id.to_string(),
                        app_id: job.app_id.to_string(),
                        app_name: job.app_name,
                        image: job.image,
                        status: job.status as i32,
                        host_id: job.host_id.unwrap_or_default().to_string(),
                        vm_id: job.vm_id.unwrap_or_default().to_string(),
                        cpu_usage,
                        ram_used_bytes,
                        user_id: job.user_id.to_string(),
                        deployment_id: job.deployment_id.unwrap_or_default().to_string(),
                        ipv6_address: job.config.ipv6_address.unwrap_or_default(),
                        tx_bytes,
                        rx_bytes,
                        hypervisor: hypervisor as i32,
                    });
                }

                Ok(apps)
            })
            .await
    }

    pub async fn list_workers(&self) -> DomainResult<Vec<WorkerInfo>> {
        let telemetry = self.ctx.telemetry.clone();
        telemetry
            .observe_result("query", "list_workers", async {
                let workers = self.ctx.worker_repo.list_workers().await?;

                Ok(workers
                    .into_iter()
                    .map(|w| WorkerInfo {
                        host_id: w.host_id.to_string(),
                        hostname: w.hostname,
                        last_heartbeat: w.last_heartbeat,
                        wireguard_pubkey: w.wireguard_pubkey.unwrap_or_default(),
                        advertise_address: w.advertise_address,
                    })
                    .collect())
            })
            .await
    }
}
