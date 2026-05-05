pub mod deployment;

pub use deployment::DeploymentService;

use crate::domain::{
    AgentClient, DomainError, DomainResult, Job, JobRepository, JobStatus, WorkerRepository,
};
use std::sync::Arc;

pub struct AppService {
    pub deployment: DeploymentService,
    pub job_repo: Arc<dyn JobRepository>,
    pub worker_repo: Arc<dyn WorkerRepository>,
    pub agent_client: Arc<dyn AgentClient>,
}

impl AppService {
    pub fn new(
        job_repo: Arc<dyn JobRepository>,
        worker_repo: Arc<dyn WorkerRepository>,
        agent_client: Arc<dyn AgentClient>,
        pool: sqlx::PgPool,
    ) -> Self {
        Self {
            deployment: DeploymentService::new(
                job_repo.clone(),
                worker_repo.clone(),
                agent_client.clone(),
                pool,
            ),
            job_repo,
            worker_repo,
            agent_client,
        }
    }

    pub async fn get_app_status(&self, job_id: &str, user_id: &str) -> DomainResult<Job> {
        let job = self
            .job_repo
            .get_job(job_id)
            .await?
            .ok_or_else(|| DomainError::JobNotFound(job_id.to_string()))?;

        if job.user_id != user_id && user_id != "system" {
            return Err(DomainError::Unauthorized(
                "You do not own this job".to_string(),
            ));
        }

        Ok(job)
    }

    pub async fn pause_app(&self, job_id: &str, user_id: &str) -> DomainResult<()> {
        let job = self.get_app_status(job_id, user_id).await?;

        if job.status == JobStatus::Stopped {
            return Ok(());
        }

        if let (Some(host_id), Some(vm_id)) = (&job.host_id, &job.vm_id) {
            if let Err(e) = self.agent_client.pause_vm(host_id, vm_id).await {
                tracing::warn!(
                    "Pause failed for {}, attempting stop fallback: {}",
                    job_id,
                    e
                );
                self.agent_client.stop_vm(host_id, vm_id).await?;
            }
            self.job_repo
                .update_job_status(job_id, JobStatus::Stopped)
                .await?;
        }
        Ok(())
    }

    pub async fn resume_app(&self, job_id: &str, user_id: &str) -> DomainResult<()> {
        let job = self.get_app_status(job_id, user_id).await?;

        // Ensure exclusivity
        self.deployment
            .deploy_app(
                // This is not quite right, we just need the exclusivity part
                job.app_id.clone(),
                job.app_name.clone(),
                job.image.clone(),
                job.user_id.clone(),
                job.deployment_id.clone().unwrap_or_default(),
                job.config.clone(),
                crate::domain::worker::SchedulingStrategy::LeastLoaded,
            )
            .await
            .ok(); // This is a bit hacky for now, ideally exclusivity is its own service

        if let (Some(host_id), Some(vm_id)) = (&job.host_id, &job.vm_id) {
            self.agent_client.resume_vm(host_id, vm_id).await?;
            self.job_repo
                .update_job_status(job_id, JobStatus::Running)
                .await?;
        }
        Ok(())
    }

    pub async fn delete_app(&self, job_id: &str, user_id: &str) -> DomainResult<()> {
        let job = self.get_app_status(job_id, user_id).await?;

        if let (Some(host_id), Some(vm_id)) = (&job.host_id, &job.vm_id) {
            let _ = self.agent_client.delete_vm(host_id, vm_id).await;
        }

        self.job_repo.remove_job(job_id).await?;
        Ok(())
    }

    pub async fn delete_all_by_app(&self, app_id: &str, user_id: &str) -> DomainResult<()> {
        let jobs = self.job_repo.list_jobs(Some(user_id), None).await?;
        let app_jobs: Vec<_> = jobs.into_iter().filter(|j| j.app_id == app_id).collect();

        for job in app_jobs {
            if let (Some(host_id), Some(vm_id)) = (&job.host_id, &job.vm_id) {
                let _ = self.agent_client.delete_vm(host_id, vm_id).await;
            }
        }

        self.job_repo.remove_jobs_by_app(app_id).await?;
        Ok(())
    }

    pub async fn get_job_metrics(&self, job: &Job) -> (f32, u64) {
        let metrics = async {
            let host_id = job.host_id.as_ref()?;
            let worker = self.worker_repo.get_worker(host_id).await.ok()??;
            let metrics = worker.metrics.as_ref()?;
            let vm_id = job.vm_id.as_ref()?;
            metrics
                .vms
                .get(vm_id)
                .map(|m| (m.cpu_usage, m.ram_used_bytes))
        }
        .await;

        metrics.unwrap_or((0.0, 0))
    }
}
