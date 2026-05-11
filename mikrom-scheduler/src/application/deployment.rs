use crate::domain::{
    AgentClient, DomainError, DomainResult, Job, JobRepository, JobStatus, SchedulingStrategy,
    VmConfig, Worker, WorkerRepository,
};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

pub struct DeploymentService {
    job_repo: Arc<dyn JobRepository>,
    worker_repo: Arc<dyn WorkerRepository>,
    agent_client: Arc<dyn AgentClient>,
}

impl DeploymentService {
    pub fn new(
        job_repo: Arc<dyn JobRepository>,
        worker_repo: Arc<dyn WorkerRepository>,
        agent_client: Arc<dyn AgentClient>,
    ) -> Self {
        Self {
            job_repo,
            worker_repo,
            agent_client,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn deploy_app(
        &self,
        app_id: String,
        app_name: String,
        image: String,
        user_id: String,
        deployment_id: String,
        vpc_ipv6_prefix: String,
        mut config: VmConfig,
        strategy: SchedulingStrategy,
    ) -> DomainResult<Job> {
        let job_id = Uuid::new_v4().to_string();
        let vm_id = Uuid::new_v4().to_string();

        // 1. Allocate 6PN IPv6 if possible
        #[allow(clippy::collapsible_if)]
        if !vpc_ipv6_prefix.is_empty() {
            if let Ok(prefix) = vpc_ipv6_prefix.parse::<std::net::Ipv6Addr>() {
                let ipv6 = mikrom_proto::sixpn::SixPn::allocate_vm_ipv6(prefix, &job_id);
                config.ipv6_address = Some(ipv6.to_string());
                config.ipv6_gateway = Some("fe80::1".to_string());
            }
        }

        // 2. Select best worker
        let worker = self.select_best_worker(&config, &app_id, strategy).await?;
        let host_id = worker.host_id.clone();

        let mut job = Job::new(
            job_id.clone(),
            app_id.clone(),
            app_name,
            image.clone(),
            config, // Move config instead of cloning
            user_id.clone(),
            Some(deployment_id),
        );
        job.schedule(host_id.clone(), vm_id.clone());

        // 2. Persist job
        self.job_repo.add_job(job.clone()).await?;

        // 4. Ensure exclusivity (Disabled for Zero-Downtime deployments)
        // if let Err(e) = self.ensure_exclusivity(&app_id, &job_id, &user_id).await {
        //     let _ = self.job_repo.remove_job(&job_id).await;
        //     return Err(e);
        // }

        tracing::info!(job_id = %job_id, host_id = %host_id, "Dispatching job to agent");

        // 5. Forward to agent
        if let Err(e) = self
            .agent_client
            .start_vm(&host_id, &app_id, &image, &vm_id, &job.config)
            .await
        {
            tracing::error!(job_id = %job_id, error = %e, "Failed to deploy to agent");
            // Remove job if we failed to start it
            let _ = self.job_repo.remove_job(&job_id).await;
            return Err(e);
        }

        // 6. Mark as running
        self.job_repo
            .start_job(&job_id, chrono::Utc::now().timestamp())
            .await?;
        job.status = JobStatus::Running;
        job.started_at = Some(chrono::Utc::now().timestamp());

        Ok(job)
    }

    async fn select_best_worker(
        &self,
        config: &VmConfig,
        app_id: &str,
        strategy: SchedulingStrategy,
    ) -> DomainResult<Worker> {
        let workers = self.worker_repo.get_available_workers(30).await?;

        if workers.is_empty() {
            return Err(DomainError::NoWorkers);
        }

        let mut viable_workers: Vec<Worker> = workers
            .into_iter()
            .filter(|w| {
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

        // Count current app instances per worker for anti-affinity
        let jobs = self.job_repo.list_jobs(None, None, None).await?;
        let mut app_counts_per_host: HashMap<String, u32> = HashMap::new();
        for job in jobs {
            #[allow(clippy::collapsible_if)]
            if job.app_id == app_id
                && job.status != JobStatus::Failed
                && job.status != JobStatus::Cancelled
            {
                if let Some(host_id) = &job.host_id {
                    *app_counts_per_host.entry(host_id.clone()).or_insert(0) += 1;
                }
            }
        }

        viable_workers.sort_by(|a, b| {
            let score_a = a.metrics.as_ref().map_or(0.0, |m| m.calculate_score(10));
            let score_b = b.metrics.as_ref().map_or(0.0, |m| m.calculate_score(10));

            let penalty_a = (*app_counts_per_host.get(&a.host_id).unwrap_or(&0) as f32) * 0.2;
            let penalty_b = (*app_counts_per_host.get(&b.host_id).unwrap_or(&0) as f32) * 0.2;

            let final_a = (score_a - penalty_a).max(0.0);
            let final_b = (score_b - penalty_b).max(0.0);

            match strategy {
                SchedulingStrategy::LeastLoaded => final_b.partial_cmp(&final_a),
                SchedulingStrategy::BinPacking => final_a.partial_cmp(&final_b),
            }
            .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(viable_workers.remove(0))
    }

    #[allow(dead_code)]
    async fn ensure_exclusivity(
        &self,
        app_id: &str,
        current_job_id: &str,
        _user_id: &str,
    ) -> DomainResult<()> {
        let jobs = self.job_repo.list_jobs(None, None, None).await?;
        let other_jobs: Vec<Job> = jobs
            .into_iter()
            .filter(|j| {
                j.app_id == app_id
                    && j.job_id != current_job_id
                    && j.status != JobStatus::Stopped
                    && j.status != JobStatus::Cancelled
                    && j.status != JobStatus::Failed
            })
            .collect();

        for old_job in other_jobs {
            tracing::info!(new_job_id = %current_job_id, old_job_id = %old_job.job_id, app_id = %app_id, "Pausing existing cluster instance for exclusivity");
            #[allow(clippy::collapsible_if)]
            if let Some(host_id) = &old_job.host_id {
                if let Some(vm_id) = &old_job.vm_id {
                    let _ = self.agent_client.pause_vm(host_id, vm_id).await;
                    let _ = self
                        .job_repo
                        .update_job_status(&old_job.job_id, JobStatus::Stopped)
                        .await;
                }
            }
        }
        Ok(())
    }
}
