use crate::application::{AppContext, publish_job_update_best_effort};
use crate::domain::{
    DomainError, DomainResult, Job, JobStatus, SchedulingStrategy, VmConfig, Worker,
};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct DeploymentService {
    ctx: Arc<AppContext>,
}

pub struct DeployAppParams {
    pub app_id: String,
    pub app_name: String,
    pub image: String,
    pub user_id: String,
    pub deployment_id: String,
    pub vpc_ipv6_prefix: String,
    pub config: VmConfig,
    pub strategy: SchedulingStrategy,
}

impl DeploymentService {
    pub fn new(ctx: Arc<AppContext>) -> Self {
        Self { ctx }
    }

    pub async fn deploy_app(&self, params: DeployAppParams) -> DomainResult<Job> {
        let telemetry = self.ctx.telemetry.clone();
        telemetry
            .observe_result("app", "deploy_app", async {
                let job_id = Uuid::new_v4().to_string();
                let vm_id = Uuid::new_v4().to_string();

                let mut config = params.config;
                config = self.apply_ipv6_assignment(config, &job_id, &params.vpc_ipv6_prefix);
                let worker = self
                    .select_best_worker(&config, &params.app_id, params.strategy)
                    .await?;
                let host_id = worker.host_id.clone();

                let mut job = Job::new(
                    job_id.clone().into(),
                    params.app_id.clone().into(),
                    params.app_name,
                    params.image.clone(),
                    config,
                    params.user_id.clone().into(),
                    Some(params.deployment_id.clone().into()),
                );
                job.schedule(host_id.to_string(), vm_id.to_string());

                self.ctx.job_repo.add_job(job.clone()).await?;

                tracing::info!(job_id = %job_id, host_id = %host_id, "Dispatching job to agent");

                if let Err(e) = self
                    .ctx
                    .agent_client
                    .start_vm(&host_id, &params.app_id, &params.image, &vm_id, &job.config)
                    .await
                {
                    tracing::error!(job_id = %job_id, error = %e, "Failed to deploy to agent");
                    if let Err(remove_err) = self.ctx.job_repo.remove_job(&job_id).await {
                        tracing::warn!(
                            job_id = %job_id,
                            error = %remove_err,
                            "Failed to roll back partially created job after deployment failure"
                        );
                    }
                    return Err(e);
                }

                if let Err(e) = self
                    .ctx
                    .job_repo
                    .start_job(&job_id, chrono::Utc::now().timestamp())
                    .await
                {
                    tracing::error!(job_id = %job_id, error = %e, "Failed to persist deployed job");
                    let host_id_string = host_id.to_string();
                    if let Err(cleanup_err) =
                        Self::rollback_failed_deploy(&self.ctx, &job_id, &host_id_string, &vm_id)
                            .await
                    {
                        tracing::warn!(
                            job_id = %job_id,
                            host_id = %host_id,
                            vm_id = %vm_id,
                            error = %cleanup_err,
                            "Rollback after start_job failure also failed"
                        );
                    }
                    return Err(e);
                }
                job.status = JobStatus::Running;
                job.started_at = Some(chrono::Utc::now().timestamp());

                // Notify cluster of new job
                publish_job_update_best_effort(
                    &self.ctx.nats_client,
                    &job,
                    "deploy-app-job-update",
                )
                .await;

                Ok(job)
            })
            .await
    }

    async fn rollback_failed_deploy(
        ctx: &Arc<AppContext>,
        job_id: &str,
        host_id: &str,
        vm_id: &str,
    ) -> DomainResult<()> {
        if let Err(e) = ctx.agent_client.delete_vm(host_id, vm_id).await {
            tracing::warn!(
                job_id = %job_id,
                host_id = %host_id,
                vm_id = %vm_id,
                error = %e,
                "Best-effort VM cleanup failed after deployment rollback"
            );
        }

        if let Err(e) = ctx.job_repo.remove_job(job_id).await {
            tracing::warn!(
                job_id = %job_id,
                error = %e,
                "Best-effort job cleanup failed after deployment rollback"
            );
        }

        Ok(())
    }

    fn apply_ipv6_assignment(
        &self,
        mut config: VmConfig,
        job_id: &str,
        vpc_ipv6_prefix: &str,
    ) -> VmConfig {
        if let Ok(prefix) = vpc_ipv6_prefix.parse::<std::net::Ipv6Addr>() {
            let ipv6 = mikrom_proto::sixpn::SixPn::allocate_vm_ipv6(prefix, job_id);
            config.ipv6_address = Some(ipv6.to_string());
            config.ipv6_gateway = Some("fe80::1".to_string());
        } else {
            // If no prefix is provided, we must clear any inherited IPv6 to avoid conflicts
            // with the template job we might have cloned.
            config.ipv6_address = None;
            config.ipv6_gateway = None;
        }

        config
    }

    fn score_worker(worker: &Worker, app_counts_per_host: &HashMap<String, u32>) -> f32 {
        let base_score = worker
            .metrics
            .as_ref()
            .map_or(0.0, |metrics| metrics.calculate_score(10));
        let penalty = (*app_counts_per_host
            .get(worker.host_id.as_ref())
            .unwrap_or(&0) as f32)
            * 0.2;
        (base_score - penalty).max(0.0)
    }

    async fn select_best_worker(
        &self,
        config: &VmConfig,
        app_id: &str,
        strategy: SchedulingStrategy,
    ) -> DomainResult<Worker> {
        let workers = self.ctx.worker_repo.get_available_workers(30).await?;

        if workers.is_empty() {
            return Err(DomainError::NoWorkers);
        }

        let mut viable_workers: Vec<Worker> = workers
            .into_iter()
            .filter(|w| {
                // When hypervisor is unspecified, accept any worker.
                // Otherwise, reject workers that do not explicitly contain the requested hypervisor.
                // Note: We assume workers with an empty list are old agents that only support Firecracker.
                if config.hypervisor != crate::domain::job::HypervisorType::Unspecified {
                    let supported = if w.supported_hypervisors.is_empty() {
                        vec![crate::domain::job::HypervisorType::Firecracker]
                    } else {
                        w.supported_hypervisors.clone()
                    };

                    if !supported.contains(&config.hypervisor) {
                        return false;
                    }
                }
                if let Some(ref metrics) = w.metrics {
                    metrics.can_fit_vm(config.memory_mib, config.disk_mib)
                } else {
                    false
                }
            })
            .collect();

        if viable_workers.is_empty() {
            return Err(DomainError::NoFit);
        }

        let app_counts_per_host = self.count_app_instances_per_host(app_id).await?;

        viable_workers.sort_by(|a, b| {
            let score_a = Self::score_worker(a, &app_counts_per_host);
            let score_b = Self::score_worker(b, &app_counts_per_host);

            match strategy {
                SchedulingStrategy::LeastLoaded => score_b.partial_cmp(&score_a),
                SchedulingStrategy::BinPacking => score_a.partial_cmp(&score_b),
            }
            .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(viable_workers.remove(0))
    }

    async fn count_app_instances_per_host(
        &self,
        app_id: &str,
    ) -> DomainResult<HashMap<String, u32>> {
        let jobs = self.ctx.job_repo.list_jobs(None, None, None).await?;
        let mut app_counts_per_host: HashMap<String, u32> = HashMap::new();

        for job in jobs {
            if job.app_id != app_id.into() {
                continue;
            }

            if matches!(job.status, JobStatus::Failed | JobStatus::Cancelled) {
                continue;
            }

            if let Some(host_id) = job.host_id {
                *app_counts_per_host.entry(host_id.to_string()).or_insert(0) += 1;
            }
        }

        Ok(app_counts_per_host)
    }
}
