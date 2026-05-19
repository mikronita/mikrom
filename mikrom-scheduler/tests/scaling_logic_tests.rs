use async_trait::async_trait;
use mikrom_proto::sixpn::SixPn;
use mikrom_scheduler::application::AppService;
use mikrom_scheduler::domain::{
    AgentClient, AppConfig, AppRepository, DomainResult, Job, JobRepository, JobStatus, VmConfig,
    Worker, WorkerRepository,
};
use std::net::Ipv6Addr;
use std::sync::Arc;

struct MockScalingAppRepo {
    apps: Vec<AppConfig>,
}

#[async_trait]
impl AppRepository for MockScalingAppRepo {
    async fn update_app_config(&self, _config: AppConfig) -> anyhow::Result<()> {
        Ok(())
    }
    async fn get_app_config(&self, app_id: &str) -> anyhow::Result<Option<AppConfig>> {
        Ok(self.apps.iter().find(|a| a.id == app_id).cloned())
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
}

struct MockScalingJobRepo {
    jobs: std::sync::Mutex<Vec<Job>>,
}

#[async_trait]
impl JobRepository for MockScalingJobRepo {
    async fn add_job(&self, job: Job) -> DomainResult<()> {
        self.jobs.lock().unwrap().push(job);
        Ok(())
    }
    async fn get_job(&self, job_id: &str) -> DomainResult<Option<Job>> {
        Ok(self
            .jobs
            .lock()
            .unwrap()
            .iter()
            .find(|j| j.job_id == job_id)
            .cloned())
    }
    async fn update_job_status(&self, job_id: &str, status: JobStatus) -> DomainResult<()> {
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(j) = jobs.iter_mut().find(|j| j.job_id == job_id) {
            j.status = status;
        }
        Ok(())
    }
    async fn start_job(&self, _j: &str, _ts: i64) -> DomainResult<()> {
        Ok(())
    }
    async fn fail_job(&self, _j: &str, _m: String, _ts: i64) -> DomainResult<()> {
        Ok(())
    }
    async fn cancel_job(&self, _j: &str, _ts: i64) -> DomainResult<()> {
        Ok(())
    }
    async fn remove_job(&self, job_id: &str) -> DomainResult<()> {
        self.jobs.lock().unwrap().retain(|j| j.job_id != job_id);
        Ok(())
    }
    async fn remove_jobs_by_app(&self, app_id: &str) -> DomainResult<()> {
        self.jobs.lock().unwrap().retain(|j| j.app_id != app_id);
        Ok(())
    }
    async fn list_jobs<'a>(
        &self,
        _u: Option<&'a str>,
        app_id: Option<&'a str>,
        status: Option<JobStatus>,
    ) -> DomainResult<Vec<Job>> {
        let jobs = self.jobs.lock().unwrap();
        Ok(jobs
            .iter()
            .filter(|j| {
                (app_id.is_none() || Some(j.app_id.as_str()) == app_id)
                    && (status.is_none() || Some(j.status) == status)
            })
            .cloned()
            .collect())
    }
    async fn find_job_by_vm_id(&self, _v: &str) -> DomainResult<Option<Job>> {
        Ok(None)
    }
}

struct MockScalingAgentClient;
#[async_trait]
impl AgentClient for MockScalingAgentClient {
    async fn update_firewall(
        &self,
        _h: &str,
        _v: &str,
        _r: Vec<mikrom_proto::scheduler::FirewallRule>,
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
        Ok(true)
    }
    async fn create_volume(&self, _h: &str, _v: &str, _s: u32, _p: &str) -> DomainResult<()> {
        Ok(())
    }
    async fn create_snapshot(&self, _h: &str, _v: &str, _sn: &str, _p: &str) -> DomainResult<()> {
        Ok(())
    }
    async fn delete_volume(&self, _h: &str, _v: &str, _p: &str) -> DomainResult<()> {
        Ok(())
    }
    async fn delete_snapshot(&self, _h: &str, _v: &str, _sn: &str, _p: &str) -> DomainResult<()> {
        Ok(())
    }
    async fn restore_snapshot(&self, _h: &str, _v: &str, _sn: &str, _p: &str) -> DomainResult<()> {
        Ok(())
    }
    async fn clone_volume(
        &self,
        _h: &str,
        _sv: &str,
        _sn: &str,
        _tv: &str,
        _p: &str,
    ) -> DomainResult<()> {
        Ok(())
    }
}

struct MockScalingWorkerRepo;
#[async_trait]
impl WorkerRepository for MockScalingWorkerRepo {
    async fn register(&self, _w: Worker) -> DomainResult<()> {
        Ok(())
    }
    async fn unregister(&self, _h: &str) -> DomainResult<()> {
        Ok(())
    }
    async fn update_metrics(
        &self,
        _h: &str,
        _m: mikrom_scheduler::domain::HostMetrics,
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
        Ok(vec![Worker {
            host_id: "host-1".to_string(),
            hostname: "host-1".to_string(),
            advertise_address: "::1".to_string(),
            wireguard_pubkey: Some("pub".to_string()),
            wireguard_ip: None,
            wireguard_port: None,
            metrics: Some(mikrom_scheduler::domain::HostMetrics {
                cpu_usage: 10.0,
                ram_total_bytes: 10 * 1024 * 1024 * 1024,
                ram_used_bytes: 1 * 1024 * 1024 * 1024,
                disk_total_bytes: 100 * 1024 * 1024 * 1024,
                disk_used_bytes: 10 * 1024 * 1024 * 1024,
                apps_count: 1,
                load_avg_1: 0.1,
                load_avg_5: 0.1,
                load_avg_15: 0.1,
                timestamp: chrono::Utc::now().timestamp(),
                vms: std::collections::HashMap::new(),
            }),
            registered_at: 0,
            last_heartbeat: chrono::Utc::now().timestamp(),
        }])
    }
}

#[tokio::test]
async fn test_reconcile_scale_up() {
    let app_id = "app-1";
    let app_config = AppConfig {
        id: app_id.to_string(),
        user_id: "user-1".to_string(),
        vpc_ipv6_prefix: "fd00::".to_string(),
        desired_replicas: 2,
        min_replicas: 1,
        max_replicas: 3,
        autoscaling_enabled: false,
        cpu_threshold: 80.0,
        mem_threshold: 80.0,
    };

    let template_job = Job::new(
        "job-0".to_string(),
        app_id.to_string(),
        "test-app".to_string(),
        "test-image".to_string(),
        VmConfig::default(),
        "user-1".to_string(),
        Some("dep-1".to_string()),
    );

    let job_repo = Arc::new(MockScalingJobRepo {
        jobs: std::sync::Mutex::new(vec![template_job.clone()]),
    });
    let app_repo = Arc::new(MockScalingAppRepo {
        apps: vec![app_config],
    });
    let worker_repo = Arc::new(MockScalingWorkerRepo);
    let agent_client = Arc::new(MockScalingAgentClient);
    let pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();

    let service = AppService::new(
        job_repo.clone(),
        app_repo.clone(),
        worker_repo.clone(),
        agent_client.clone(),
        pool,
    );

    job_repo
        .update_job_status("job-0", JobStatus::Running)
        .await
        .unwrap();

    service.scale_app(app_id, 2, "user-1").await.unwrap();

    let final_jobs = job_repo.list_jobs(None, Some(app_id), None).await.unwrap();
    assert_eq!(final_jobs.len(), 2, "Should have scaled up to 2 jobs");
}

#[tokio::test]
async fn test_reconcile_scale_down() {
    let app_id = "app-1";
    let app_config = AppConfig {
        id: app_id.to_string(),
        user_id: "user-1".to_string(),
        vpc_ipv6_prefix: "fd00::".to_string(),
        desired_replicas: 1,
        min_replicas: 1,
        max_replicas: 3,
        autoscaling_enabled: false,
        cpu_threshold: 80.0,
        mem_threshold: 80.0,
    };

    let job1 = Job::new(
        "job-1".to_string(),
        app_id.to_string(),
        "app".to_string(),
        "img".to_string(),
        VmConfig::default(),
        "user-1".to_string(),
        None,
    );
    let mut job2 = Job::new(
        "job-2".to_string(),
        app_id.to_string(),
        "app".to_string(),
        "img".to_string(),
        VmConfig::default(),
        "user-1".to_string(),
        None,
    );
    job2.status = JobStatus::Running;

    let job_repo = Arc::new(MockScalingJobRepo {
        jobs: std::sync::Mutex::new(vec![job1, job2]),
    });
    let app_repo = Arc::new(MockScalingAppRepo {
        apps: vec![app_config],
    });
    let worker_repo = Arc::new(MockScalingWorkerRepo);
    let agent_client = Arc::new(MockScalingAgentClient);
    let pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();

    let service = AppService::new(
        job_repo.clone(),
        app_repo.clone(),
        worker_repo.clone(),
        agent_client.clone(),
        pool,
    );

    service.scale_app(app_id, 1, "user-1").await.unwrap();

    let final_jobs = job_repo.list_jobs(None, Some(app_id), None).await.unwrap();
    assert_eq!(final_jobs.len(), 1, "Should have scaled down to 1 job");
}

#[test]
fn test_ipam_uniqueness_for_replicas() {
    let prefix = Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 0);

    let ip1 = SixPn::allocate_vm_ipv6(prefix, "job-1");
    let ip2 = SixPn::allocate_vm_ipv6(prefix, "job-2");
    let ip3 = SixPn::allocate_vm_ipv6(prefix, "job-3");

    assert_ne!(ip1, ip2, "IPs for different jobs must be unique");
    assert_ne!(ip2, ip3);
    assert_ne!(ip1, ip3);

    let ip1_octets = ip1.octets();
    assert_eq!(ip1_octets[0], 0xfd);
    assert_eq!(ip1_octets[1], 0);
    assert_eq!(ip1_octets[2], 0);
    assert_eq!(ip1_octets[3], 0);
    assert_eq!(ip1_octets[4], 0);
    assert_eq!(ip1_octets[15], 1);
}
