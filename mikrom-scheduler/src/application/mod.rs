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

        if matches!(job.status, JobStatus::Paused | JobStatus::Stopped) {
            tracing::info!(
                job_id = %job_id,
                status = %job.status.as_str(),
                "Pause requested for a job that is already paused or stopped"
            );
            return Ok(());
        }

        tracing::info!(
            job_id = %job_id,
            app_id = %job.app_id,
            vm_id = ?job.vm_id,
            "Pausing job"
        );

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
                .update_job_status(job_id, JobStatus::Paused)
                .await?;
        }

        tracing::info!(job_id = %job_id, "Job paused successfully");
        Ok(())
    }

    pub async fn resume_app(&self, job_id: &str, user_id: &str) -> DomainResult<()> {
        let job = self.get_app_status(job_id, user_id).await?;

        tracing::info!(
            job_id = %job_id,
            app_id = %job.app_id,
            vm_id = ?job.vm_id,
            "Resuming job"
        );

        if let (Some(host_id), Some(vm_id)) = (&job.host_id, &job.vm_id) {
            self.agent_client.resume_vm(host_id, vm_id).await?;
            self.job_repo
                .update_job_status(job_id, JobStatus::Running)
                .await?;
        }

        tracing::info!(job_id = %job_id, "Job resumed successfully");
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
        let mut failures = Vec::new();

        for job in app_jobs {
            #[allow(clippy::collapsible_if)]
            if let (Some(host_id), Some(vm_id)) = (&job.host_id, &job.vm_id) {
                if let Err(e) = self.agent_client.delete_vm(host_id, vm_id).await {
                    let error_text = e.to_string();
                    if Self::is_vm_already_gone(&error_text) {
                        tracing::info!(
                            vm_id = %vm_id,
                            host_id = %host_id,
                            "VM already absent during app cleanup; treating as success"
                        );
                        continue;
                    }

                    tracing::error!("Failed to delete VM {} on host {}: {}", vm_id, host_id, e);
                    failures.push(format!("{} on {}: {}", vm_id, host_id, e));
                }
            }
        }

        if !failures.is_empty() {
            return Err(crate::domain::DomainError::Infrastructure(format!(
                "Failed to delete one or more VMs for app {app_id}: {}",
                failures.join("; ")
            )));
        }

        self.job_repo.remove_jobs_by_app(app_id).await?;
        Ok(())
    }

    fn is_vm_already_gone(error_text: &str) -> bool {
        let normalized = error_text.to_lowercase();
        normalized.contains("vm not found")
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
        _req: mikrom_proto::scheduler::UpdateSecurityGroupsRequest,
    ) -> DomainResult<()> {
        // ... (rest of implementation) ...
        Ok(())
    }

    pub async fn create_volume(
        &self,
        host_id: &str,
        volume_id: &str,
        size_mib: u32,
        pool_name: &str,
    ) -> DomainResult<()> {
        let target_host = if host_id.is_empty() {
            // If no host specified, pick a random active worker
            let workers = self
                .worker_repo
                .list_workers()
                .await
                .map_err(|e| DomainError::Infrastructure(e.to_string()))?;
            workers
                .first()
                .ok_or_else(|| DomainError::Infrastructure("No active workers".to_string()))?
                .host_id
                .clone()
        } else {
            host_id.to_string()
        };

        self.agent_client
            .create_volume(&target_host, volume_id, size_mib, pool_name)
            .await
    }

    pub async fn create_snapshot(
        &self,
        host_id: &str,
        volume_id: &str,
        snapshot_name: &str,
        pool_name: &str,
    ) -> DomainResult<()> {
        let target_host = if host_id.is_empty() {
            self.pick_any_healthy_worker().await?
        } else {
            host_id.to_string()
        };

        self.agent_client
            .create_snapshot(&target_host, volume_id, snapshot_name, pool_name)
            .await
    }

    pub async fn delete_volume(
        &self,
        host_id: &str,
        volume_id: &str,
        pool_name: &str,
    ) -> DomainResult<()> {
        let target_host = if host_id.is_empty() {
            self.pick_any_healthy_worker().await?
        } else {
            host_id.to_string()
        };

        self.agent_client
            .delete_volume(&target_host, volume_id, pool_name)
            .await
    }

    pub async fn delete_snapshot(
        &self,
        host_id: &str,
        volume_id: &str,
        snapshot_name: &str,
        pool_name: &str,
    ) -> DomainResult<()> {
        let target_host = if host_id.is_empty() {
            self.pick_any_healthy_worker().await?
        } else {
            host_id.to_string()
        };

        self.agent_client
            .delete_snapshot(&target_host, volume_id, snapshot_name, pool_name)
            .await
    }

    pub async fn restore_snapshot(
        &self,
        host_id: &str,
        volume_id: &str,
        snapshot_name: &str,
        pool_name: &str,
    ) -> DomainResult<()> {
        let target_host = if host_id.is_empty() {
            self.pick_any_healthy_worker().await?
        } else {
            host_id.to_string()
        };

        self.agent_client
            .restore_snapshot(&target_host, volume_id, snapshot_name, pool_name)
            .await
    }

    pub async fn clone_volume(
        &self,
        host_id: &str,
        source_volume_id: &str,
        snapshot_name: &str,
        target_volume_id: &str,
        pool_name: &str,
    ) -> DomainResult<()> {
        let target_host = if host_id.is_empty() {
            self.pick_any_healthy_worker().await?
        } else {
            host_id.to_string()
        };

        self.agent_client
            .clone_volume(
                &target_host,
                source_volume_id,
                snapshot_name,
                target_volume_id,
                pool_name,
            )
            .await
    }

    async fn pick_any_healthy_worker(&self) -> DomainResult<String> {
        let workers = self.worker_repo.get_available_workers(30).await?;
        if let Some(w) = workers.first() {
            return Ok(w.host_id.clone());
        }

        // Fallback: Try any worker that has sent a heartbeat recently, even if it hasn't sent metrics yet
        let all_workers = self.worker_repo.list_workers().await?;
        let now = chrono::Utc::now().timestamp();
        let fallback = all_workers
            .iter()
            .filter(|w| now - w.last_heartbeat < 30)
            .max_by_key(|w| w.last_heartbeat);

        fallback
            .map(|w| w.host_id.clone())
            .ok_or_else(|| DomainError::Infrastructure("No healthy workers available for storage operation. Ensure agents are running and connected to NATS.".to_string()))
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
    use crate::domain::worker::{MockAgentClient, MockJobRepository, MockWorkerRepository};
    use crate::domain::{
        AgentClient, DomainError, DomainResult, JobRepository, Worker, WorkerRepository,
    };
    use async_trait::async_trait;
    use mockall::predicate::eq;
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

        async fn create_volume(
            &self,
            _host_id: &str,
            _volume_id: &str,
            _size_mib: u32,
            _pool_name: &str,
        ) -> DomainResult<()> {
            Ok(())
        }

        async fn create_snapshot(
            &self,
            _host_id: &str,
            _volume_id: &str,
            _snapshot_name: &str,
            _pool_name: &str,
        ) -> DomainResult<()> {
            Ok(())
        }

        async fn delete_volume(
            &self,
            _host_id: &str,
            _volume_id: &str,
            _pool_name: &str,
        ) -> DomainResult<()> {
            Ok(())
        }

        async fn delete_snapshot(
            &self,
            _host_id: &str,
            _volume_id: &str,
            _snapshot_name: &str,
            _pool_name: &str,
        ) -> DomainResult<()> {
            Ok(())
        }

        async fn restore_snapshot(
            &self,
            _host_id: &str,
            _volume_id: &str,
            _snapshot_name: &str,
            _pool_name: &str,
        ) -> DomainResult<()> {
            Ok(())
        }

        async fn clone_volume(
            &self,
            _host_id: &str,
            _source_volume_id: &str,
            _snapshot_name: &str,
            _target_volume_id: &str,
            _pool_name: &str,
        ) -> DomainResult<()> {
            Ok(())
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

    fn paused_job() -> Job {
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
        job.status = JobStatus::Running;
        job
    }

    #[tokio::test]
    async fn test_pause_app_success_updates_status_without_stop() {
        let job = paused_job();
        let mut job_repo = MockJobRepository::new();
        job_repo.expect_get_job().with(eq("job-1")).returning({
            let job = job.clone();
            move |_| Ok(Some(job.clone()))
        });
        job_repo
            .expect_update_job_status()
            .with(eq("job-1"), eq(JobStatus::Paused))
            .times(1)
            .returning(|_, _| Ok(()));

        let mut agent_client = MockAgentClient::new();
        agent_client
            .expect_pause_vm()
            .with(eq("host-1"), eq("vm-1"))
            .times(1)
            .returning(|_, _| Ok(()));
        agent_client.expect_stop_vm().times(0);

        let worker_repo = Arc::new(MockWorkerRepository::new());
        let job_repo = Arc::new(job_repo);
        let agent_client = Arc::new(agent_client);
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

        service.pause_app("job-1", "user-1").await.unwrap();
    }

    #[tokio::test]
    async fn test_pause_app_fallback_stops_vm_on_pause_failure() {
        let job = paused_job();
        let mut job_repo = MockJobRepository::new();
        job_repo.expect_get_job().with(eq("job-1")).returning({
            let job = job.clone();
            move |_| Ok(Some(job.clone()))
        });
        job_repo
            .expect_update_job_status()
            .with(eq("job-1"), eq(JobStatus::Paused))
            .times(1)
            .returning(|_, _| Ok(()));

        let mut agent_client = MockAgentClient::new();
        agent_client
            .expect_pause_vm()
            .with(eq("host-1"), eq("vm-1"))
            .times(1)
            .returning(|_, _| Err(DomainError::Infrastructure("boom".to_string())));
        agent_client
            .expect_stop_vm()
            .with(eq("host-1"), eq("vm-1"))
            .times(1)
            .returning(|_, _| Ok(()));

        let worker_repo = Arc::new(MockWorkerRepository::new());
        let job_repo = Arc::new(job_repo);
        let agent_client = Arc::new(agent_client);
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

        service.pause_app("job-1", "user-1").await.unwrap();
    }

    #[tokio::test]
    async fn test_delete_all_by_app_treats_missing_vm_as_success() {
        let job = paused_job();
        let mut job_repo = MockJobRepository::new();
        job_repo
            .expect_list_jobs()
            .returning(move |_, _, _| Ok(vec![job.clone()]));
        job_repo
            .expect_remove_jobs_by_app()
            .with(eq("app-1"))
            .times(1)
            .returning(|_| Ok(()));

        let mut agent_client = MockAgentClient::new();
        agent_client
            .expect_delete_vm()
            .with(eq("host-1"), eq("vm-1"))
            .times(1)
            .returning(|_, _| {
                Err(DomainError::Infrastructure(
                    "VM not found: vm-1".to_string(),
                ))
            });

        let worker_repo = Arc::new(MockWorkerRepository::new());
        let job_repo = Arc::new(job_repo);
        let agent_client = Arc::new(agent_client);
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

        service.delete_all_by_app("app-1", "user-1").await.unwrap();
    }

    #[tokio::test]
    async fn test_delete_all_by_app_returns_error_when_vm_delete_fails() {
        let job = paused_job();
        let mut job_repo = MockJobRepository::new();
        job_repo
            .expect_list_jobs()
            .returning(move |_, _, _| Ok(vec![job.clone()]));
        job_repo
            .expect_remove_jobs_by_app()
            .with(eq("app-1"))
            .times(0)
            .returning(|_| Ok(()));

        let mut agent_client = MockAgentClient::new();
        agent_client
            .expect_delete_vm()
            .with(eq("host-1"), eq("vm-1"))
            .times(1)
            .returning(|_, _| Err(DomainError::Infrastructure("boom".to_string())));

        let worker_repo = Arc::new(MockWorkerRepository::new());
        let job_repo = Arc::new(job_repo);
        let agent_client = Arc::new(agent_client);
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

        let err = service
            .delete_all_by_app("app-1", "user-1")
            .await
            .expect_err("cleanup should fail");

        assert!(matches!(err, DomainError::Infrastructure(_)));
    }
}
