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
    pub pool: sqlx::PgPool,
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
            ),
            job_repo,
            worker_repo,
            agent_client,
            pool,
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
                String::new(), // VPC prefix unknown during resume for now
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
        let jobs = self.job_repo.list_jobs(Some(user_id), None, None).await?;
        let app_jobs: Vec<_> = jobs.into_iter().filter(|j| j.app_id == app_id).collect();

        for job in app_jobs {
            if let (Some(host_id), Some(vm_id)) = (&job.host_id, &job.vm_id)
                && let Err(e) = self.agent_client.delete_vm(host_id, vm_id).await
            {
                tracing::error!("Failed to delete VM {} on host {}: {}", vm_id, host_id, e);
            }
        }

        self.job_repo.remove_jobs_by_app(app_id).await?;
        Ok(())
    }

    pub async fn check_health(&self, job_id: &str, user_id: &str) -> DomainResult<bool> {
        let job = self.get_app_status(job_id, user_id).await?;
        if let (Some(host_id), Some(vm_id)) = (&job.host_id, &job.vm_id) {
            self.agent_client.check_health(host_id, vm_id).await
        } else {
            Ok(false)
        }
    }

    pub async fn update_security_groups(
        &self,
        req: mikrom_proto::scheduler::UpdateSecurityGroupsRequest,
    ) -> DomainResult<()> {
        // 1. Fetch rules from DB
        let rules_rows = sqlx::query(
            "SELECT protocol, port_start, port_end, action FROM security_rules WHERE app_id = $1 ORDER BY priority ASC",
        )
        .bind(uuid::Uuid::parse_str(&req.app_id).map_err(|e| DomainError::Infrastructure(e.to_string()))?)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

        let proto_rules: Vec<mikrom_proto::scheduler::FirewallRule> = rules_rows
            .into_iter()
            .map(|r| {
                use sqlx::Row;
                mikrom_proto::scheduler::FirewallRule {
                    protocol: r.get("protocol"),
                    port_start: r.get("port_start"),
                    port_end: r.get("port_end"),
                    action: r.get("action"),
                }
            })
            .collect();

        // 2. Find all active jobs for this app
        let app_jobs = self
            .job_repo
            .list_jobs(None, Some(&req.app_id), Some(JobStatus::Running))
            .await?;

        // 3. Push updates to agents
        for job in app_jobs {
            if let (Some(host_id), Some(vm_id)) = (&job.host_id, &job.vm_id) {
                let _ = self
                    .agent_client
                    .update_firewall(host_id, vm_id, proto_rules.clone())
                    .await;
            }
        }

        Ok(())
    }

    pub async fn get_job_metrics(&self, job: &Job) -> (f32, u64, u64, u64) {
        let metrics = async {
            let host_id = job.host_id.as_ref()?;
            let worker = self.worker_repo.get_worker(host_id).await.ok()??;
            let metrics = worker.metrics.as_ref()?;
            let vm_id = job.vm_id.as_ref()?;
            metrics
                .vms
                .get(vm_id)
                .map(|m| (m.cpu_usage, m.ram_used_bytes, m.tx_bytes, m.rx_bytes))
        }
        .await;

        metrics.unwrap_or((0.0, 0, 0, 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::job::{Job, JobStatus, VmConfig};
    use crate::domain::{AgentClient, DomainResult, JobRepository, Worker, WorkerRepository};
    use async_trait::async_trait;
    use std::sync::Arc;

    struct DummyJobRepo {
        job: Job,
    }

    #[async_trait]
    impl JobRepository for DummyJobRepo {
        async fn add_job(&self, _job: Job) -> DomainResult<()> {
            Ok(())
        }
        async fn get_job(&self, _job_id: &str) -> DomainResult<Option<Job>> {
            Ok(Some(self.job.clone()))
        }
        async fn update_job_status(&self, _job_id: &str, _status: JobStatus) -> DomainResult<()> {
            Ok(())
        }
        async fn start_job(&self, _job_id: &str, _ts: i64) -> DomainResult<()> {
            Ok(())
        }
        async fn fail_job(&self, _job_id: &str, _msg: String, _ts: i64) -> DomainResult<()> {
            Ok(())
        }
        async fn cancel_job(&self, _j: &str, _ts: i64) -> DomainResult<()> {
            Ok(())
        }
        async fn remove_job(&self, _j: &str) -> DomainResult<()> {
            Ok(())
        }
        async fn remove_jobs_by_app(&self, _app: &str) -> DomainResult<()> {
            Ok(())
        }
        async fn list_jobs<'a>(
            &self,
            _u: Option<&'a str>,
            _a: Option<&'a str>,
            _s: Option<JobStatus>,
        ) -> DomainResult<Vec<Job>> {
            Ok(vec![])
        }
        async fn find_job_by_vm_id(&self, _v: &str) -> DomainResult<Option<Job>> {
            Ok(None)
        }
    }

    struct DummyWorkerRepo;
    #[async_trait]
    impl WorkerRepository for DummyWorkerRepo {
        async fn register(&self, _w: Worker) -> DomainResult<()> {
            Ok(())
        }
        async fn unregister(&self, _h: &str) -> DomainResult<()> {
            Ok(())
        }
        async fn update_metrics(
            &self,
            _h: &str,
            _m: crate::domain::HostMetrics,
        ) -> DomainResult<()> {
            Ok(())
        }
        async fn get_worker(&self, _h: &str) -> DomainResult<Option<Worker>> {
            Ok(None)
        }
        async fn list_workers(&self) -> DomainResult<Vec<Worker>> {
            Ok(vec![])
        }
        async fn get_available_workers(&self, _t: i64) -> DomainResult<Vec<Worker>> {
            Ok(vec![])
        }
    }

    struct DummyAgentClient {
        healthy: bool,
    }

    #[async_trait]
    impl AgentClient for DummyAgentClient {
        async fn update_firewall(
            &self,
            _host_id: &str,
            _vm_id: &str,
            _rules: Vec<mikrom_proto::scheduler::FirewallRule>,
        ) -> DomainResult<()> {
            Ok(())
        }

        async fn start_vm(
            &self,
            _h: &str,
            _a: &str,
            _i: &str,
            _v: &str,
            _c: &VmConfig,
        ) -> DomainResult<()> {
            Ok(())
        }
        async fn pause_vm(&self, _h: &str, _v: &str) -> DomainResult<()> {
            Ok(())
        }
        async fn resume_vm(&self, _h: &str, _v: &str) -> DomainResult<()> {
            Ok(())
        }
        async fn stop_vm(&self, _h: &str, _v: &str) -> DomainResult<()> {
            Ok(())
        }
        async fn delete_vm(&self, _h: &str, _v: &str) -> DomainResult<()> {
            Ok(())
        }
        async fn check_health(&self, _h: &str, _v: &str) -> DomainResult<bool> {
            Ok(self.healthy)
        }
    }

    #[tokio::test]
    async fn test_check_health_dispatch() {
        let mut job = Job::new(
            "job-1".to_string(),
            "app-1".to_string(),
            "app1".to_string(),
            "img".to_string(),
            VmConfig::default(),
            "user-1".to_string(),
            None,
        );
        job.schedule("host-1".to_string(), "vm-1".to_string());

        let job_repo = Arc::new(DummyJobRepo { job });
        let worker_repo = Arc::new(DummyWorkerRepo);
        let agent_client = Arc::new(DummyAgentClient { healthy: true });

        // Use a lazy pool that doesn't connect for testing
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();

        let service = AppService {
            deployment: DeploymentService::new(
                job_repo.clone(),
                worker_repo.clone(),
                agent_client.clone(),
            ),
            job_repo,
            worker_repo,
            agent_client,
            pool,
        };

        let res = service.check_health("job-1", "user-1").await.unwrap();
        assert!(res);
    }
}
