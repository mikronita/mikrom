use async_trait::async_trait;
use mikrom_scheduler::application::{AppService, SchedulerRuntimeConfig};
use mikrom_scheduler::domain::{
    AgentClient, AppConfig, AppId, AppRepository, DeploymentId, DomainResult, HostId,
    HypervisorType, Job, JobId, JobRepository, JobStatus, UserId, VmConfig, VmId, Worker,
    WorkerRepository,
};
use std::sync::Arc;
use tokio::sync::Mutex;

async fn connect_nats_or_skip() -> Option<async_nats::Client> {
    match async_nats::connect("nats://localhost:4223").await {
        Ok(client) => Some(client),
        Err(err) => {
            eprintln!("Skipping scheduler scaling test: failed to connect to NATS: {err}");
            None
        },
    }
}

fn test_runtime() -> SchedulerRuntimeConfig {
    SchedulerRuntimeConfig {
        router_idle_timeout_secs: 900,
        worker_stale_threshold_secs: 60,
        restore_retry_backoff_secs: 3600,
    }
}

struct MockScalingAppRepo {
    apps: Vec<AppConfig>,
}

#[async_trait]
impl AppRepository for MockScalingAppRepo {
    async fn update_app_config(&self, _: AppConfig) -> anyhow::Result<()> {
        Ok(())
    }
    async fn get_app_config(&self, app_id: &str) -> anyhow::Result<Option<AppConfig>> {
        Ok(self.apps.iter().find(|a| a.id.as_ref() == app_id).cloned())
    }
    async fn get_app_config_by_hostname(
        &self,
        hostname: &str,
    ) -> anyhow::Result<Option<AppConfig>> {
        Ok(self.apps.iter().find(|a| a.hostname == hostname).cloned())
    }
    async fn list_all_apps(&self) -> anyhow::Result<Vec<AppConfig>> {
        Ok(self.apps.clone())
    }
    async fn list_autoscaling_apps(&self) -> anyhow::Result<Vec<AppConfig>> {
        Ok(self
            .apps
            .iter()
            .filter(|a| a.autoscaling_enabled)
            .cloned()
            .collect())
    }
    async fn remove_app_config(&self, _: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn remove_app_and_jobs_by_app(&self, _: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

struct MockScalingJobRepo {
    jobs: Arc<Mutex<Vec<Job>>>,
}

#[async_trait]
impl JobRepository for MockScalingJobRepo {
    async fn add_job(&self, job: Job) -> DomainResult<()> {
        self.jobs.lock().await.push(job);
        Ok(())
    }
    async fn get_job(&self, job_id: &str) -> DomainResult<Option<Job>> {
        Ok(self
            .jobs
            .lock()
            .await
            .iter()
            .find(|j| j.job_id.as_ref() == job_id)
            .cloned())
    }
    async fn update_job_status(&self, job_id: &str, status: JobStatus) -> DomainResult<()> {
        if let Some(job) = self
            .jobs
            .lock()
            .await
            .iter_mut()
            .find(|j| j.job_id.as_ref() == job_id)
        {
            job.status = status;
        }
        Ok(())
    }
    async fn start_job(&self, job_id: &str, _: i64) -> DomainResult<()> {
        if let Some(job) = self
            .jobs
            .lock()
            .await
            .iter_mut()
            .find(|j| j.job_id.as_ref() == job_id)
        {
            job.status = JobStatus::Running;
        }
        Ok(())
    }
    async fn fail_job(&self, job_id: &str, _: String, _: i64) -> DomainResult<()> {
        if let Some(job) = self
            .jobs
            .lock()
            .await
            .iter_mut()
            .find(|j| j.job_id.as_ref() == job_id)
        {
            job.status = JobStatus::Failed;
        }
        Ok(())
    }
    async fn cancel_job(&self, job_id: &str, _: i64) -> DomainResult<()> {
        if let Some(job) = self
            .jobs
            .lock()
            .await
            .iter_mut()
            .find(|j| j.job_id.as_ref() == job_id)
        {
            job.status = JobStatus::Cancelled;
        }
        Ok(())
    }
    async fn remove_job(&self, job_id: &str) -> DomainResult<()> {
        self.jobs
            .lock()
            .await
            .retain(|j| j.job_id.as_ref() != job_id);
        Ok(())
    }
    async fn remove_jobs_by_app(&self, app_id: &str) -> DomainResult<()> {
        self.jobs
            .lock()
            .await
            .retain(|j| j.app_id.as_ref() != app_id);
        Ok(())
    }
    async fn list_jobs<'a>(
        &self,
        _: Option<&'a str>,
        app_id: Option<&'a str>,
        status: Option<JobStatus>,
    ) -> DomainResult<Vec<Job>> {
        let jobs = self.jobs.lock().await;
        Ok(jobs
            .iter()
            .filter(|j| {
                let app_match = app_id.map(|id| j.app_id.as_ref() == id).unwrap_or(true);
                let status_match = status.map(|s| j.status == s).unwrap_or(true);
                app_match && status_match
            })
            .cloned()
            .collect())
    }
    async fn find_job_by_vm_id(&self, vm_id: &str) -> DomainResult<Option<Job>> {
        Ok(self
            .jobs
            .lock()
            .await
            .iter()
            .find(|j| j.vm_id.as_deref() == Some(vm_id))
            .cloned())
    }
}

struct MockScalingWorkerRepo;

#[async_trait]
impl WorkerRepository for MockScalingWorkerRepo {
    async fn register(&self, _: Worker) -> DomainResult<()> {
        Ok(())
    }
    async fn unregister(&self, _: &str) -> DomainResult<()> {
        Ok(())
    }
    async fn update_metrics(
        &self,
        _: &str,
        _: mikrom_scheduler::domain::HostMetrics,
    ) -> DomainResult<()> {
        Ok(())
    }
    async fn get_worker(&self, _: &str) -> DomainResult<Option<Worker>> {
        Ok(Some(Worker {
            host_id: HostId::from("host-1".to_string()),
            hostname: "host1".to_string(),
            advertise_address: "1.1.1.1".to_string(),
            wireguard_pubkey: None,
            wireguard_ip: None,
            wireguard_port: None,
            metrics: None,
            registered_at: 0,
            last_heartbeat: chrono::Utc::now().timestamp(),
            status: mikrom_scheduler::domain::WorkerStatus::Online,
            supported_hypervisors: vec![],
        }))
    }
    async fn list_workers(&self) -> DomainResult<Vec<Worker>> {
        Ok(vec![Worker {
            host_id: HostId::from("host-1".to_string()),
            hostname: "host1".to_string(),
            advertise_address: "1.1.1.1".to_string(),
            wireguard_pubkey: None,
            wireguard_ip: None,
            wireguard_port: None,
            metrics: None,
            registered_at: 0,
            last_heartbeat: chrono::Utc::now().timestamp(),
            status: mikrom_scheduler::domain::WorkerStatus::Online,
            supported_hypervisors: vec![],
        }])
    }
    async fn get_available_workers(&self, _: i64) -> DomainResult<Vec<Worker>> {
        self.list_workers().await
    }

    async fn mark_stale_workers_offline(&self, _: i64) -> DomainResult<u64> {
        Ok(0)
    }
}

struct MockScalingAgentClient;

#[async_trait]
impl AgentClient for MockScalingAgentClient {
    async fn start_vm(&self, _: &str, _: &str, _: &str, _: &str, _: &VmConfig) -> DomainResult<()> {
        Ok(())
    }
    async fn pause_vm(&self, _: &str, _: &str) -> DomainResult<()> {
        Ok(())
    }
    async fn resume_vm(&self, _: &str, _: &str) -> DomainResult<()> {
        Ok(())
    }
    async fn stop_vm(&self, _: &str, _: &str) -> DomainResult<()> {
        Ok(())
    }
    async fn delete_vm(&self, _: &str, _: &str, _: HypervisorType) -> DomainResult<()> {
        Ok(())
    }
    async fn check_health(&self, _: &str, _: &str) -> DomainResult<bool> {
        Ok(true)
    }
    async fn update_firewall(
        &self,
        _: &str,
        _: &str,
        _: Vec<mikrom_proto::scheduler::FirewallRule>,
    ) -> DomainResult<()> {
        Ok(())
    }
    async fn create_volume(&self, _: &str, _: &str, _: u32, _: &str) -> DomainResult<()> {
        Ok(())
    }
    async fn create_snapshot(&self, _: &str, _: &str, _: &str, _: &str) -> DomainResult<()> {
        Ok(())
    }
    async fn delete_volume(&self, _: &str, _: &str, _: &str) -> DomainResult<()> {
        Ok(())
    }
    async fn delete_snapshot(&self, _: &str, _: &str, _: &str, _: &str) -> DomainResult<()> {
        Ok(())
    }
    async fn restore_snapshot(&self, _: &str, _: &str, _: &str, _: &str) -> DomainResult<()> {
        Ok(())
    }
    async fn clone_volume(&self, _: &str, _: &str, _: &str, _: &str, _: &str) -> DomainResult<()> {
        Ok(())
    }
    async fn vm_snapshot_create(&self, _: &str, _: &str, _: &str) -> DomainResult<()> {
        Ok(())
    }
    async fn vm_snapshot_restore(&self, _: &str, _: &str, _: &str) -> DomainResult<()> {
        Ok(())
    }
    async fn vm_snapshot_delete(&self, _: &str, _: &str, _: &str) -> DomainResult<()> {
        Ok(())
    }
    async fn vm_snapshot_list(
        &self,
        _: &str,
        _: &str,
    ) -> DomainResult<Vec<mikrom_proto::agent::VmSnapshotInfo>> {
        Ok(vec![])
    }
    async fn attach_volume(&self, _: &str, _: &str, _: &str, _: &str, _: bool) -> DomainResult<()> {
        Ok(())
    }
    async fn detach_volume(&self, _: &str, _: &str, _: &str) -> DomainResult<()> {
        Ok(())
    }
    async fn start_migration(&self, _: &str, _: &str, _: &str, _: &str) -> DomainResult<()> {
        Ok(())
    }
    async fn cancel_migration(&self, _: &str, _: &str) -> DomainResult<()> {
        Ok(())
    }
    async fn query_migration(&self, _: &str, _: &str) -> DomainResult<String> {
        Ok("completed".to_string())
    }
    async fn set_balloon(&self, _: &str, _: &str, _: u32) -> DomainResult<()> {
        Ok(())
    }
    async fn query_balloon(&self, _: &str, _: &str) -> DomainResult<(u32, u32)> {
        Ok((512, 512))
    }
}

#[tokio::test]
async fn test_reconcile_scale_up_from_zero_manual() {
    let app_id = "app-1";
    let app_config = AppConfig {
        id: AppId::from(app_id.to_string()),
        user_id: UserId::from("user-1".to_string()),
        vpc_ipv6_prefix: "fd00::".to_string(),
        hostname: "app1.example.com".to_string(),
        desired_replicas: 1,
        min_replicas: 0, // Should stay at 0 without traffic
        max_replicas: 3,
        autoscaling_enabled: false,
        cpu_threshold: 80.0,
        mem_threshold: 80.0,
        last_router_traffic_at: 0,
        last_scaled_to_zero_at: 0,
        restore_retry_after_at: 0,
    };

    let job_repo = MockScalingJobRepo {
        jobs: Arc::new(Mutex::new(vec![])),
    };
    let app_repo = MockScalingAppRepo {
        apps: vec![app_config],
    };
    let worker_repo = Arc::new(MockScalingWorkerRepo);
    let agent_client = Arc::new(MockScalingAgentClient);
    let pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();
    let Some(nats_client) = connect_nats_or_skip().await else {
        return;
    };

    let service = AppService::new(
        Arc::new(job_repo),
        Arc::new(app_repo),
        worker_repo,
        agent_client,
        nats_client,
        pool,
        test_runtime(),
    );

    service.reconcile_apps().await.unwrap();

    let jobs = service
        .job_repo
        .list_jobs(None, Some(app_id), None)
        .await
        .unwrap();
    assert_eq!(
        jobs.len(),
        0,
        "Should stay at 0 replicas because min_replicas is 0 and no traffic"
    );
}

#[tokio::test]
async fn test_reconcile_scale_up_from_zero_with_traffic() {
    let app_id = "app-1";
    let now = chrono::Utc::now().timestamp();
    let app_config = AppConfig {
        id: AppId::from(app_id.to_string()),
        user_id: UserId::from("user-1".to_string()),
        vpc_ipv6_prefix: "fd00::".to_string(),
        hostname: "app1.example.com".to_string(),
        desired_replicas: 1,
        min_replicas: 0,
        max_replicas: 3,
        autoscaling_enabled: false,
        cpu_threshold: 80.0,
        mem_threshold: 80.0,
        last_router_traffic_at: now,
        last_scaled_to_zero_at: now - 10, // Traffic is newer
        restore_retry_after_at: 0,
    };

    let paused_job = Job {
        job_id: JobId::from("job-1".to_string()),
        app_id: AppId::from(app_id.to_string()),
        app_name: "app1".to_string(),
        image: "img".to_string(),
        user_id: UserId::from("user-1".to_string()),
        status: JobStatus::Paused,
        host_id: Some(HostId::from("host-1".to_string())),
        vm_id: Some(VmId::from("vm-1".to_string())),
        created_at: now - 100,
        started_at: None,
        stopped_at: None,
        scheduled_at: None,
        deployment_id: Some(DeploymentId::from("dep-1".to_string())),
        config: VmConfig::default(),
        error_message: None,
    };

    let job_repo = MockScalingJobRepo {
        jobs: Arc::new(Mutex::new(vec![paused_job])),
    };
    let app_repo = MockScalingAppRepo {
        apps: vec![app_config],
    };
    let worker_repo = Arc::new(MockScalingWorkerRepo);
    let agent_client = Arc::new(MockScalingAgentClient);
    let pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();
    let Some(nats_client) = connect_nats_or_skip().await else {
        return;
    };

    let service = AppService::new(
        Arc::new(job_repo),
        Arc::new(app_repo),
        worker_repo,
        agent_client,
        nats_client,
        pool,
        test_runtime(),
    );

    service.reconcile_apps().await.unwrap();

    let jobs = service
        .job_repo
        .list_jobs(None, Some(app_id), None)
        .await
        .unwrap();
    let running_jobs: Vec<_> = jobs
        .iter()
        .filter(|j| j.status == JobStatus::Running)
        .collect();
    assert_eq!(
        running_jobs.len(),
        1,
        "Should have resumed the job due to traffic"
    );
}

#[tokio::test]
async fn test_reconcile_skips_restore_during_backoff() {
    let app_id = "app-1";
    let now = chrono::Utc::now().timestamp();
    let app_config = AppConfig {
        id: AppId::from(app_id.to_string()),
        user_id: UserId::from("user-1".to_string()),
        vpc_ipv6_prefix: "fd00::".to_string(),
        hostname: "app1.example.com".to_string(),
        desired_replicas: 1,
        min_replicas: 0,
        max_replicas: 3,
        autoscaling_enabled: false,
        cpu_threshold: 80.0,
        mem_threshold: 80.0,
        last_router_traffic_at: now,
        last_scaled_to_zero_at: now - 10,
        restore_retry_after_at: now + 300,
    };

    let paused_job = Job {
        job_id: JobId::from("job-1".to_string()),
        app_id: AppId::from(app_id.to_string()),
        app_name: "app1".to_string(),
        image: "img".to_string(),
        user_id: UserId::from("user-1".to_string()),
        status: JobStatus::Paused,
        host_id: Some(HostId::from("host-1".to_string())),
        vm_id: Some(VmId::from("vm-1".to_string())),
        created_at: now - 100,
        started_at: None,
        stopped_at: None,
        scheduled_at: None,
        deployment_id: Some(DeploymentId::from("dep-1".to_string())),
        config: VmConfig::default(),
        error_message: None,
    };

    let job_repo = MockScalingJobRepo {
        jobs: Arc::new(Mutex::new(vec![paused_job.clone()])),
    };
    let app_repo = MockScalingAppRepo {
        apps: vec![app_config],
    };
    let worker_repo = Arc::new(MockScalingWorkerRepo);
    let agent_client = Arc::new(MockScalingAgentClient);
    let pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();
    let Some(nats_client) = connect_nats_or_skip().await else {
        return;
    };

    let service = AppService::new(
        Arc::new(job_repo),
        Arc::new(app_repo),
        worker_repo,
        agent_client,
        nats_client,
        pool,
        test_runtime(),
    );

    service.reconcile_apps().await.unwrap();

    let jobs = service
        .job_repo
        .list_jobs(None, Some(app_id), None)
        .await
        .unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].status, JobStatus::Paused);
}
