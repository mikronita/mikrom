use async_trait::async_trait;
use mikrom_proto::router::RouterTrafficEvent;
use mikrom_proto::subjects;
use mikrom_scheduler::application::AppService;
use mikrom_scheduler::domain::{
    AgentClient, AppConfig, AppRepository, DomainResult, Job, JobRepository, JobStatus, VmConfig,
    Worker, WorkerRepository,
};
use mikrom_scheduler::infrastructure::db::{PgAppRepository, PgJobRepository, PgWorkerRepository};
use mikrom_scheduler::infrastructure::nats::NatsEventLoop;
use mikrom_scheduler::server::SchedulerServer;
use prost::Message;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{Barrier, Mutex};
use tokio::time::Duration;

#[path = "common_utils.rs"]
mod common_utils;

#[derive(Clone)]
struct InMemoryAppRepo {
    app: Arc<Mutex<AppConfig>>,
}

#[async_trait]
impl AppRepository for InMemoryAppRepo {
    async fn update_app_config(&self, config: AppConfig) -> anyhow::Result<()> {
        *self.app.lock().await = config;
        Ok(())
    }

    async fn get_app_config(&self, app_id: &str) -> anyhow::Result<Option<AppConfig>> {
        let app = self.app.lock().await;
        Ok((app.id == app_id).then_some(app.clone()))
    }

    async fn get_app_config_by_hostname(
        &self,
        hostname: &str,
    ) -> anyhow::Result<Option<AppConfig>> {
        let app = self.app.lock().await;
        Ok((app.hostname == hostname).then_some(app.clone()))
    }

    async fn list_all_apps(&self) -> anyhow::Result<Vec<AppConfig>> {
        Ok(vec![self.app.lock().await.clone()])
    }

    async fn list_autoscaling_apps(&self) -> anyhow::Result<Vec<AppConfig>> {
        Ok(vec![])
    }

    async fn remove_app_config(&self, _: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

struct BarrierAppRepo {
    app: Arc<Mutex<AppConfig>>,
    barrier: Arc<Barrier>,
    update_calls: Arc<AtomicUsize>,
}

#[async_trait]
impl AppRepository for BarrierAppRepo {
    async fn update_app_config(&self, config: AppConfig) -> anyhow::Result<()> {
        *self.app.lock().await = config;
        self.update_calls.fetch_add(1, Ordering::SeqCst);
        self.barrier.wait().await;
        Ok(())
    }

    async fn get_app_config(&self, app_id: &str) -> anyhow::Result<Option<AppConfig>> {
        let app = self.app.lock().await;
        Ok((app.id == app_id).then_some(app.clone()))
    }

    async fn get_app_config_by_hostname(
        &self,
        hostname: &str,
    ) -> anyhow::Result<Option<AppConfig>> {
        let app = self.app.lock().await;
        Ok((app.hostname == hostname).then_some(app.clone()))
    }

    async fn list_all_apps(&self) -> anyhow::Result<Vec<AppConfig>> {
        Ok(vec![self.app.lock().await.clone()])
    }

    async fn list_autoscaling_apps(&self) -> anyhow::Result<Vec<AppConfig>> {
        Ok(vec![])
    }

    async fn remove_app_config(&self, _: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Clone)]
struct InMemoryJobRepo {
    jobs: Arc<Mutex<Vec<Job>>>,
}

#[async_trait]
impl JobRepository for InMemoryJobRepo {
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
            .find(|job| job.job_id == job_id)
            .cloned())
    }

    async fn update_job_status(&self, job_id: &str, status: JobStatus) -> DomainResult<()> {
        let mut jobs = self.jobs.lock().await;
        if let Some(job) = jobs.iter_mut().find(|job| job.job_id == job_id) {
            job.status = status;
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
        self.jobs.lock().await.retain(|job| job.job_id != job_id);
        Ok(())
    }

    async fn remove_jobs_by_app(&self, app_id: &str) -> DomainResult<()> {
        self.jobs.lock().await.retain(|job| job.app_id != app_id);
        Ok(())
    }

    async fn list_jobs<'a>(
        &self,
        user_id: Option<&'a str>,
        app_id: Option<&'a str>,
        status: Option<JobStatus>,
    ) -> DomainResult<Vec<Job>> {
        let jobs = self.jobs.lock().await;
        Ok(jobs
            .iter()
            .filter(|job| {
                (user_id.is_none() || Some(job.user_id.as_str()) == user_id)
                    && (app_id.is_none() || Some(job.app_id.as_str()) == app_id)
                    && (status.is_none() || Some(job.status) == status)
            })
            .cloned()
            .collect())
    }

    async fn find_job_by_vm_id(&self, _v: &str) -> DomainResult<Option<Job>> {
        Ok(None)
    }
}

#[derive(Clone, Default)]
struct InMemoryWorkerRepo;

#[async_trait]
impl WorkerRepository for InMemoryWorkerRepo {
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
        Ok(vec![])
    }
}

#[derive(Clone, Default)]
struct RecordingAgentClient {
    resume_calls: Arc<AtomicUsize>,
    start_calls: Arc<AtomicUsize>,
}

#[async_trait]
impl AgentClient for RecordingAgentClient {
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
        self.start_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn pause_vm(&self, _h: &str, _v: &str) -> DomainResult<()> {
        Ok(())
    }

    async fn resume_vm(&self, _h: &str, _v: &str) -> DomainResult<()> {
        self.resume_calls.fetch_add(1, Ordering::SeqCst);
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

#[tokio::test]
async fn test_router_traffic_restores_paused_deployment() {
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let client = match async_nats::connect(&nats_url).await {
        Ok(client) => client,
        Err(err) => {
            eprintln!("Skipping integration test: unable to connect to NATS at {nats_url}: {err}");
            return;
        },
    };

    let app_id = format!("app-restore-{}", uuid::Uuid::new_v4());
    let hostname = format!("{}.example.com", uuid::Uuid::new_v4());
    let user_id = "user-1".to_string();

    let app_repo = InMemoryAppRepo {
        app: Arc::new(Mutex::new(AppConfig {
            id: app_id.clone(),
            user_id: user_id.clone(),
            vpc_ipv6_prefix: "fd00::".to_string(),
            hostname: hostname.clone(),
            desired_replicas: 1,
            min_replicas: 0,
            max_replicas: 3,
            autoscaling_enabled: false,
            cpu_threshold: 80.0,
            mem_threshold: 80.0,
            last_router_traffic_at: 0,
            last_scaled_to_zero_at: 1,
        })),
    };

    let paused_job = Job {
        job_id: "job-1".to_string(),
        app_id: app_id.clone(),
        app_name: "restore-app".to_string(),
        image: "test-image".to_string(),
        user_id: user_id.clone(),
        status: JobStatus::Paused,
        host_id: Some("host-1".to_string()),
        vm_id: Some("vm-1".to_string()),
        scheduled_at: None,
        started_at: None,
        stopped_at: None,
        error_message: None,
        created_at: chrono::Utc::now().timestamp() - 600,
        deployment_id: Some("dep-1".to_string()),
        config: VmConfig::default(),
    };

    let job_repo = InMemoryJobRepo {
        jobs: Arc::new(Mutex::new(vec![paused_job])),
    };

    let worker_repo = InMemoryWorkerRepo;
    let agent_client = RecordingAgentClient::default();
    let pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();

    let app_service = Arc::new(AppService::new(
        Arc::new(job_repo.clone()),
        Arc::new(app_repo.clone()),
        Arc::new(worker_repo),
        Arc::new(agent_client.clone()),
        client.clone(),
        pool,
    ));

    let server = SchedulerServer::new(app_service.clone(), None);
    let event_loop = NatsEventLoop::new(server, client.clone())
        .with_queue_group(format!("test-group-{}", uuid::Uuid::new_v4()));
    let loop_handle = tokio::spawn(async move {
        let _ = event_loop.run().await;
    });

    tokio::time::sleep(Duration::from_millis(250)).await;

    let initial_jobs = job_repo
        .list_jobs(Some(&user_id), Some(&app_id), None)
        .await
        .unwrap();
    assert_eq!(
        initial_jobs.len(),
        1,
        "Test setup should start with a single paused job"
    );

    let event = RouterTrafficEvent {
        hostname: hostname.clone(),
        router_id: "router-1".to_string(),
        timestamp: chrono::Utc::now().timestamp(),
    };

    client
        .publish(subjects::ROUTER_TRAFFIC_EVENT, event.encode_to_vec().into())
        .await
        .expect("Failed to publish router traffic event");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let jobs = job_repo
            .list_jobs(Some(&user_id), Some(&app_id), None)
            .await
            .unwrap();
        let app = app_repo.app.lock().await.clone();

        if jobs.iter().any(|job| job.status == JobStatus::Running) && app.last_router_traffic_at > 0
        {
            assert_eq!(
                jobs.len(),
                1,
                "Router traffic should resume the existing deployment, not create a new one"
            );
            assert_eq!(agent_client.start_calls.load(Ordering::SeqCst), 0);
            assert_eq!(agent_client.resume_calls.load(Ordering::SeqCst), 1);
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            loop_handle.abort();
            panic!("Timed out waiting for router traffic to restore the paused deployment");
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    loop_handle.abort();
}

#[tokio::test]
async fn test_router_traffic_restores_paused_deployment_with_real_db() {
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let client = match async_nats::connect(&nats_url).await {
        Ok(client) => client,
        Err(err) => {
            eprintln!("Skipping integration test: unable to connect to NATS at {nats_url}: {err}");
            return;
        },
    };

    let db = common_utils::TestDb::new().await;
    let pool = db.pool().clone();

    let app_repo = Arc::new(PgAppRepository::new(pool.clone()));
    let job_repo = Arc::new(PgJobRepository::new(pool.clone()));
    let worker_repo = Arc::new(PgWorkerRepository::new(pool.clone()));
    let agent_client = Arc::new(RecordingAgentClient::default());

    let app_id = format!("app-restore-real-{}", uuid::Uuid::new_v4());
    let user_id = "user-1".to_string();
    let hostname = format!("restore-real-{}.example.com", uuid::Uuid::new_v4());
    let host_id = format!("host-{}", uuid::Uuid::new_v4());
    let vm_id = format!("vm-{}", uuid::Uuid::new_v4());

    let app_config = AppConfig {
        id: app_id.clone(),
        user_id: user_id.clone(),
        vpc_ipv6_prefix: "fd00::".to_string(),
        hostname: hostname.clone(),
        desired_replicas: 1,
        min_replicas: 0,
        max_replicas: 3,
        autoscaling_enabled: false,
        cpu_threshold: 80.0,
        mem_threshold: 80.0,
        last_router_traffic_at: 0,
        last_scaled_to_zero_at: 1,
    };
    app_repo.update_app_config(app_config).await.unwrap();

    worker_repo
        .register(Worker {
            host_id: host_id.clone(),
            hostname: "worker-1".to_string(),
            advertise_address: "127.0.0.1".to_string(),
            wireguard_pubkey: Some("pub".to_string()),
            wireguard_ip: None,
            wireguard_port: Some(51820),
            metrics: None,
            registered_at: chrono::Utc::now().timestamp(),
            last_heartbeat: chrono::Utc::now().timestamp(),
        })
        .await
        .unwrap();

    let mut job = Job::new(
        "job-real-1".to_string(),
        app_id.clone(),
        "restore-app".to_string(),
        "test-image".to_string(),
        VmConfig::default(),
        user_id.clone(),
        Some("dep-1".to_string()),
    );
    job.status = JobStatus::Paused;
    job.host_id = Some(host_id.clone());
    job.vm_id = Some(vm_id.clone());
    job_repo.add_job(job).await.unwrap();

    let app_service = Arc::new(AppService::new(
        job_repo.clone(),
        app_repo.clone(),
        worker_repo.clone(),
        agent_client.clone(),
        client.clone(),
        pool,
    ));

    let server = SchedulerServer::new(app_service.clone(), None);
    let event_loop = NatsEventLoop::new(server, client.clone())
        .with_queue_group(format!("test-group-{}", uuid::Uuid::new_v4()));
    let loop_handle = tokio::spawn(async move {
        let _ = event_loop.run().await;
    });

    tokio::time::sleep(Duration::from_millis(250)).await;

    let event = RouterTrafficEvent {
        hostname: hostname.clone(),
        router_id: "router-1".to_string(),
        timestamp: chrono::Utc::now().timestamp(),
    };

    client
        .publish(subjects::ROUTER_TRAFFIC_EVENT, event.encode_to_vec().into())
        .await
        .expect("Failed to publish router traffic event");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let job = job_repo.get_job("job-real-1").await.unwrap().unwrap();
        let app = app_repo.get_app_config(&app_id).await.unwrap().unwrap();

        if job.status == JobStatus::Running
            && app.last_router_traffic_at > 0
            && agent_client.resume_calls.load(Ordering::SeqCst) == 1
            && agent_client.start_calls.load(Ordering::SeqCst) == 0
        {
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            loop_handle.abort();
            panic!("Timed out waiting for real DB restore flow");
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let final_job = job_repo.get_job("job-real-1").await.unwrap().unwrap();
    assert_eq!(final_job.status, JobStatus::Running);
    assert_eq!(
        job_repo
            .list_jobs(Some(&user_id), Some(&app_id), None)
            .await
            .unwrap()
            .len(),
        1
    );

    loop_handle.abort();
}

#[tokio::test]
async fn test_router_traffic_restore_is_deduplicated_under_concurrency() {
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let client = match async_nats::connect(&nats_url).await {
        Ok(client) => client,
        Err(err) => {
            eprintln!("Skipping integration test: unable to connect to NATS at {nats_url}: {err}");
            return;
        },
    };

    let app_id = format!("app-restore-race-{}", uuid::Uuid::new_v4());
    let hostname = format!("restore-race-{}.example.com", uuid::Uuid::new_v4());
    let user_id = "user-1".to_string();

    let update_calls = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(2));
    let app_repo = BarrierAppRepo {
        app: Arc::new(Mutex::new(AppConfig {
            id: app_id.clone(),
            user_id: user_id.clone(),
            vpc_ipv6_prefix: "fd00::".to_string(),
            hostname: hostname.clone(),
            desired_replicas: 1,
            min_replicas: 0,
            max_replicas: 3,
            autoscaling_enabled: false,
            cpu_threshold: 80.0,
            mem_threshold: 80.0,
            last_router_traffic_at: 0,
            last_scaled_to_zero_at: 1,
        })),
        barrier: barrier.clone(),
        update_calls: update_calls.clone(),
    };

    let job = {
        let mut job = Job::new(
            "job-race-1".to_string(),
            app_id.clone(),
            "race-app".to_string(),
            "race-image".to_string(),
            VmConfig::default(),
            user_id.clone(),
            Some("dep-race".to_string()),
        );
        job.status = JobStatus::Paused;
        job.host_id = Some("host-race".to_string());
        job.vm_id = Some("vm-race".to_string());
        job
    };

    let job_repo = InMemoryJobRepo {
        jobs: Arc::new(Mutex::new(vec![job])),
    };
    let worker_repo = InMemoryWorkerRepo;
    let agent_client = RecordingAgentClient::default();
    let pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();

    let service = AppService::new(
        Arc::new(job_repo.clone()),
        Arc::new(app_repo),
        Arc::new(worker_repo),
        Arc::new(agent_client.clone()),
        client.clone(),
        pool,
    );

    let server = SchedulerServer {
        app_service: Arc::new(service),
        certs: None,
    };

    let event_loop = NatsEventLoop::new(server, client.clone())
        .with_queue_group(format!("test-group-{}", uuid::Uuid::new_v4()));
    let handle = tokio::spawn(async move {
        let _ = event_loop.run().await;
    });
    tokio::time::sleep(Duration::from_millis(250)).await;

    let event = RouterTrafficEvent {
        hostname: hostname.clone(),
        router_id: "router-1".to_string(),
        timestamp: chrono::Utc::now().timestamp(),
    };

    let mut buf = Vec::new();
    event.encode(&mut buf).unwrap();
    let payload = buf.clone();
    client
        .publish(mikrom_proto::subjects::ROUTER_TRAFFIC_EVENT, buf.into())
        .await
        .unwrap();
    client
        .publish(mikrom_proto::subjects::ROUTER_TRAFFIC_EVENT, payload.into())
        .await
        .unwrap();

    // The handler is blocked on the barrier inside update_app_config.
    // We expect TWO calls to update_app_config (one for each event).
    // The test thread must call wait() once for each call to update_app_config
    // to let the background tasks proceed.

    // Wait for the first update
    let _ = barrier.wait().await;
    // Wait for the second update
    let _ = barrier.wait().await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let resume_calls = agent_client.resume_calls.load(Ordering::SeqCst);
        let running_jobs = job_repo
            .list_jobs(Some(&user_id), Some(&app_id), Some(JobStatus::Running))
            .await
            .unwrap();
        if resume_calls == 1 && running_jobs.len() == 1 && update_calls.load(Ordering::SeqCst) == 2
        {
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!(
                "Timed out waiting for deduplicated restore: resume_calls={}, running_jobs={}, update_calls={}",
                resume_calls,
                running_jobs.len(),
                update_calls.load(Ordering::SeqCst)
            );
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert_eq!(agent_client.resume_calls.load(Ordering::SeqCst), 1);
    assert_eq!(agent_client.start_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        job_repo
            .list_jobs(Some(&user_id), Some(&app_id), None)
            .await
            .unwrap()
            .len(),
        1
    );

    handle.abort();
}
