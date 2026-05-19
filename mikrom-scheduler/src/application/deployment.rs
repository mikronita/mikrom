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

        config = self.apply_ipv6_assignment(config, &job_id, &vpc_ipv6_prefix);
        let worker = self.select_best_worker(&config, &app_id, strategy).await?;
        let host_id = worker.host_id.clone();

        let mut job = self.build_job(
            job_id.clone(),
            app_id.clone(),
            app_name,
            image.clone(),
            config,
            user_id.clone(),
            deployment_id,
        );
        job.schedule(host_id.clone(), vm_id.clone());

        self.job_repo.add_job(job.clone()).await?;

        tracing::info!(job_id = %job_id, host_id = %host_id, "Dispatching job to agent");

        if let Err(e) = self
            .agent_client
            .start_vm(&host_id, &app_id, &image, &vm_id, &job.config)
            .await
        {
            tracing::error!(job_id = %job_id, error = %e, "Failed to deploy to agent");
            let _ = self.job_repo.remove_job(&job_id).await;
            return Err(e);
        }

        self.job_repo
            .start_job(&job_id, chrono::Utc::now().timestamp())
            .await?;
        job.status = JobStatus::Running;
        job.started_at = Some(chrono::Utc::now().timestamp());

        Ok(job)
    }

    #[allow(clippy::too_many_arguments)]
    fn build_job(
        &self,
        job_id: String,
        app_id: String,
        app_name: String,
        image: String,
        config: VmConfig,
        user_id: String,
        deployment_id: String,
    ) -> Job {
        Job::new(
            job_id,
            app_id,
            app_name,
            image,
            config,
            user_id,
            Some(deployment_id),
        )
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
        let penalty = (*app_counts_per_host.get(&worker.host_id).unwrap_or(&0) as f32) * 0.2;
        (base_score - penalty).max(0.0)
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
        let jobs = self.job_repo.list_jobs(None, None, None).await?;
        let mut app_counts_per_host: HashMap<String, u32> = HashMap::new();

        for job in jobs {
            if job.app_id != app_id {
                continue;
            }

            if matches!(job.status, JobStatus::Failed | JobStatus::Cancelled) {
                continue;
            }

            if let Some(host_id) = job.host_id {
                *app_counts_per_host.entry(host_id).or_insert(0) += 1;
            }
        }

        Ok(app_counts_per_host)
    }
}
