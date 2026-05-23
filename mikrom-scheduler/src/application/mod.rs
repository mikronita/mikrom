pub mod deployment;
pub mod heartbeats;
pub mod lifecycle;
pub mod query;
pub mod router_restore;
pub mod scaling;

pub use deployment::DeploymentService;
pub use heartbeats::HeartbeatService;
pub use lifecycle::JobLifecycleService;
pub use query::AppQueryService;
pub use router_restore::RouterRestoreService;
pub use scaling::ScalingService;

use crate::domain::{
    AgentClient, AppConfig, AppRepository, DomainError, DomainResult, Job, JobRepository,
    WorkerRepository,
};
use crate::infrastructure::telemetry::SchedulerTelemetry;
use mikrom_proto::agent::VmFailureEvent;
use mikrom_proto::router::RouterTrafficEvent;
use mikrom_proto::scheduler::{RouterHeartbeat, WorkerHeartbeat};
use prost::Message;
use std::ops::Deref;
use std::sync::Arc;

const AUTOSCALING_SCALE_DOWN_HYSTERESIS_RATIO: f64 = 0.5;

#[derive(Debug, Clone, Copy)]
pub struct SchedulerRuntimeConfig {
    pub router_idle_timeout_secs: i64,
    pub worker_stale_threshold_secs: i64,
    pub restore_retry_backoff_secs: i64,
}

pub(super) fn autoscale_next_replicas(
    app: &AppConfig,
    current_count: u32,
    avg_cpu: f32,
    avg_mem: f32,
) -> u32 {
    let mut desired = current_count;
    let cpu = avg_cpu as f64;
    let mem = avg_mem as f64;

    if cpu > app.cpu_threshold || mem > app.mem_threshold {
        desired = desired.saturating_add(1).min(app.max_replicas);
    } else if cpu < app.cpu_threshold * AUTOSCALING_SCALE_DOWN_HYSTERESIS_RATIO
        && mem < app.mem_threshold * AUTOSCALING_SCALE_DOWN_HYSTERESIS_RATIO
        && desired > app.min_replicas
    {
        desired -= 1;
    }

    desired
}

pub(super) async fn update_app_config_best_effort(
    app_repo: &Arc<dyn AppRepository>,
    app: AppConfig,
    context: &'static str,
) {
    let app_id = app.id.to_string();
    if let Err(e) = app_repo.update_app_config(app).await {
        tracing::warn!(
            %context,
            app_id = %app_id,
            error = %e,
            "Best-effort app config update failed"
        );
    }
}

pub(super) async fn publish_job_update_best_effort(
    nats_client: &async_nats::Client,
    job: &Job,
    context: &'static str,
) {
    use mikrom_proto::scheduler::AppInfo;

    let info = AppInfo {
        job_id: job.job_id.to_string(),
        app_id: job.app_id.to_string(),
        app_name: job.app_name.clone(),
        image: job.image.clone(),
        status: job.status as i32,
        host_id: job.host_id.clone().unwrap_or_default().to_string(),
        vm_id: job.vm_id.clone().unwrap_or_default().to_string(),
        user_id: job.user_id.to_string(),
        deployment_id: job.deployment_id.clone().unwrap_or_default().to_string(),
        ipv6_address: job.config.ipv6_address.clone().unwrap_or_default(),
        ..Default::default()
    };

    let mut buf = Vec::new();
    if let Err(e) = info.encode(&mut buf) {
        tracing::warn!(
            %context,
            job_id = %job.job_id,
            app_id = %job.app_id,
            error = %e,
            "Skipping job update publish: failed to encode payload"
        );
        return;
    }

    if let Err(e) = nats_client
        .publish(mikrom_proto::subjects::SCHEDULER_JOB_UPDATES, buf.into())
        .await
    {
        tracing::warn!(
            %context,
            job_id = %job.job_id,
            app_id = %job.app_id,
            error = %e,
            "Best-effort job update publish failed"
        );
    }
}

#[derive(Clone)]
pub struct AppContext {
    pub job_repo: Arc<dyn JobRepository>,
    pub app_repo: Arc<dyn AppRepository>,
    pub worker_repo: Arc<dyn WorkerRepository>,
    pub agent_client: Arc<dyn AgentClient>,
    pub nats_client: async_nats::Client,
    pub telemetry: SchedulerTelemetry,
    pub runtime: SchedulerRuntimeConfig,
}

impl AppContext {
    fn new(
        job_repo: Arc<dyn JobRepository>,
        app_repo: Arc<dyn AppRepository>,
        worker_repo: Arc<dyn WorkerRepository>,
        agent_client: Arc<dyn AgentClient>,
        nats_client: async_nats::Client,
        runtime: SchedulerRuntimeConfig,
    ) -> Self {
        Self {
            job_repo,
            app_repo,
            worker_repo,
            agent_client,
            nats_client,
            telemetry: SchedulerTelemetry::default(),
            runtime,
        }
    }
}

impl Deref for AppService {
    type Target = AppContext;

    fn deref(&self) -> &Self::Target {
        &self.context
    }
}

pub struct AppService {
    pub deployment: DeploymentService,
    pub context: Arc<AppContext>,
    pub queries: AppQueryService,
    pub heartbeats: HeartbeatService,
    router_restore: RouterRestoreService,
    lifecycle: JobLifecycleService,
    scaling: ScalingService,
}

impl AppService {
    pub fn new(
        job_repo: Arc<dyn JobRepository>,
        app_repo: Arc<dyn AppRepository>,
        worker_repo: Arc<dyn WorkerRepository>,
        agent_client: Arc<dyn AgentClient>,
        nats_client: async_nats::Client,
        _pool: sqlx::PgPool,
        runtime: SchedulerRuntimeConfig,
    ) -> Self {
        let context = Arc::new(AppContext::new(
            job_repo,
            app_repo,
            worker_repo,
            agent_client,
            nats_client,
            runtime,
        ));

        let heartbeats = HeartbeatService::new(context.clone());
        let deployment = DeploymentService::new(context.clone());
        let queries = AppQueryService::new(context.clone());
        let lifecycle = JobLifecycleService::new(context.clone());
        let scaling = ScalingService::new(context.clone(), deployment.clone(), lifecycle.clone());
        let router_restore = RouterRestoreService::new(context.clone(), scaling.clone());

        Self {
            deployment,
            context,
            queries,
            heartbeats,
            router_restore,
            lifecycle,
            scaling,
        }
    }

    pub async fn process_worker_heartbeat(&self, heartbeat: WorkerHeartbeat) -> DomainResult<()> {
        self.heartbeats.process_worker_heartbeat(heartbeat).await
    }

    pub async fn process_router_heartbeat(&self, heartbeat: RouterHeartbeat) -> DomainResult<()> {
        self.heartbeats.process_router_heartbeat(heartbeat).await
    }

    pub async fn process_router_traffic(&self, event: RouterTrafficEvent) -> DomainResult<()> {
        self.router_restore.process_router_traffic(event).await
    }

    pub async fn process_vm_failure(&self, event: VmFailureEvent) -> DomainResult<()> {
        self.heartbeats.process_vm_failure(event).await
    }

    pub async fn cleanup_stale_workers(&self) -> DomainResult<u64> {
        self.heartbeats.cleanup_stale_workers().await
    }

    pub async fn get_app_status(&self, job_id: &str, user_id: &str) -> DomainResult<Job> {
        self.queries.get_app_status(job_id, user_id).await
    }

    pub async fn pause_app(&self, job_id: &str, user_id: &str) -> DomainResult<()> {
        self.lifecycle.pause_app(job_id, user_id).await
    }

    pub async fn resume_app(&self, job_id: &str, user_id: &str) -> DomainResult<bool> {
        self.lifecycle.resume_app(job_id, user_id).await
    }

    pub async fn delete_app(&self, job_id: &str, user_id: &str) -> DomainResult<()> {
        self.lifecycle.delete_app(job_id, user_id).await
    }

    pub async fn delete_all_by_app(&self, app_id: &str, user_id: &str) -> DomainResult<()> {
        self.lifecycle.delete_all_by_app(app_id, user_id).await
    }

    pub async fn scale_app(
        &self,
        app_id: &str,
        desired_replicas: u32,
        user_id: &str,
    ) -> DomainResult<()> {
        self.scaling
            .scale_app(app_id, desired_replicas, user_id)
            .await
    }

    pub async fn start_autoscaler(self: Arc<Self>) {
        self.scaling.start_autoscaler().await
    }

    pub async fn reconcile_apps(&self) -> DomainResult<()> {
        self.scaling.reconcile_apps().await
    }

    async fn resolve_storage_host(&self, host_id: &str) -> DomainResult<String> {
        if !host_id.is_empty() {
            return Ok(host_id.to_string());
        }

        self.pick_any_healthy_worker().await
    }

    async fn resolve_volume_host(&self, host_id: &str) -> DomainResult<String> {
        if !host_id.is_empty() {
            return Ok(host_id.to_string());
        }

        self.pick_any_healthy_worker().await
    }

    pub async fn check_health(&self, job_id: &str, user_id: &str) -> DomainResult<bool> {
        self.queries.check_health(job_id, user_id).await
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
        let target_host = self.resolve_volume_host(host_id).await?;

        self.context
            .agent_client
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
        let target_host = self.resolve_storage_host(host_id).await?;

        self.context
            .agent_client
            .create_snapshot(&target_host, volume_id, snapshot_name, pool_name)
            .await
    }

    pub async fn delete_volume(
        &self,
        host_id: &str,
        volume_id: &str,
        pool_name: &str,
    ) -> DomainResult<()> {
        let target_host = self.resolve_storage_host(host_id).await?;

        self.context
            .agent_client
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
        let target_host = self.resolve_storage_host(host_id).await?;

        self.context
            .agent_client
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
        let target_host = self.resolve_storage_host(host_id).await?;

        self.context
            .agent_client
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
        let target_host = self.resolve_storage_host(host_id).await?;

        self.context
            .agent_client
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
        let workers = self.context.worker_repo.get_available_workers(30).await?;
        if let Some(w) = workers.first() {
            return Ok(w.host_id.to_string());
        }

        // Fallback: Try any worker that has sent a heartbeat recently, even if it hasn't sent metrics yet
        let all_workers = self.context.worker_repo.list_workers().await?;
        let now = chrono::Utc::now().timestamp();
        let fallback = all_workers
            .iter()
            .filter(|w| now - w.last_heartbeat < 30)
            .max_by_key(|w| w.last_heartbeat);

        fallback
            .map(|w| w.host_id.to_string())
            .ok_or_else(|| DomainError::Infrastructure("No healthy workers available for storage operation. Ensure agents are running and connected to NATS.".to_string()))
    }

    pub async fn get_job_metrics(&self, job: &Job) -> (f32, u64, u64, u64) {
        self.queries.get_job_metrics(job).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::app::MockAppRepository;
    use crate::domain::job::{Job, JobStatus, VmConfig};
    use crate::domain::worker::{MockAgentClient, MockJobRepository, MockWorkerRepository};
    use crate::domain::{
        AgentClient, AppConfig, AppRepository, DomainError, DomainResult, JobRepository, Worker,
        WorkerRepository,
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
        async fn cancel_job(&self, _job_id: &str, _ts: i64) -> DomainResult<()> {
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
        async fn mark_stale_workers_offline(&self, _t: i64) -> DomainResult<u64> {
            Ok(0)
        }
    }

    struct DummyAppRepo;
    #[async_trait]
    impl AppRepository for DummyAppRepo {
        async fn update_app_config(&self, _config: AppConfig) -> anyhow::Result<()> {
            Ok(())
        }
        async fn get_app_config(&self, _: &str) -> anyhow::Result<Option<AppConfig>> {
            Ok(None)
        }
        async fn get_app_config_by_hostname(&self, _: &str) -> anyhow::Result<Option<AppConfig>> {
            Ok(None)
        }
        async fn list_all_apps(&self) -> anyhow::Result<Vec<AppConfig>> {
            Ok(vec![])
        }
        async fn list_autoscaling_apps(&self) -> anyhow::Result<Vec<AppConfig>> {
            Ok(vec![])
        }
        async fn remove_app_config(&self, _: &str) -> anyhow::Result<()> {
            Ok(())
        }

        async fn remove_app_and_jobs_by_app(&self, _: &str) -> anyhow::Result<()> {
            Ok(())
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

        async fn vm_snapshot_create(
            &self,
            _host_id: &str,
            _vm_id: &str,
            _snapshot_name: &str,
        ) -> DomainResult<()> {
            Ok(())
        }
        async fn vm_snapshot_restore(
            &self,
            _host_id: &str,
            _vm_id: &str,
            _snapshot_name: &str,
        ) -> DomainResult<()> {
            Ok(())
        }
        async fn vm_snapshot_delete(
            &self,
            _host_id: &str,
            _vm_id: &str,
            _snapshot_name: &str,
        ) -> DomainResult<()> {
            Ok(())
        }
        async fn vm_snapshot_list(
            &self,
            _host_id: &str,
            _vm_id: &str,
        ) -> DomainResult<Vec<mikrom_proto::agent::VmSnapshotInfo>> {
            Ok(vec![])
        }
        async fn attach_volume(
            &self,
            _host_id: &str,
            _vm_id: &str,
            _volume_id: &str,
            _mount_point: &str,
            _read_only: bool,
        ) -> DomainResult<()> {
            Ok(())
        }
        async fn detach_volume(
            &self,
            _host_id: &str,
            _vm_id: &str,
            _volume_id: &str,
        ) -> DomainResult<()> {
            Ok(())
        }
        async fn start_migration(
            &self,
            _host_id: &str,
            _vm_id: &str,
            _target_host: &str,
            _target_uri: &str,
        ) -> DomainResult<()> {
            Ok(())
        }
        async fn cancel_migration(&self, _host_id: &str, _vm_id: &str) -> DomainResult<()> {
            Ok(())
        }
        async fn query_migration(&self, _host_id: &str, _vm_id: &str) -> DomainResult<String> {
            Ok("completed".to_string())
        }
        async fn set_balloon(
            &self,
            _host_id: &str,
            _vm_id: &str,
            _target_memory_mib: u32,
        ) -> DomainResult<()> {
            Ok(())
        }
        async fn query_balloon(&self, _host_id: &str, _vm_id: &str) -> DomainResult<(u32, u32)> {
            Ok((512, 1024))
        }
    }

    async fn connect_nats_or_skip() -> Option<async_nats::Client> {
        match async_nats::connect("nats://localhost:4223").await {
            Ok(client) => Some(client),
            Err(err) => {
                eprintln!("Skipping scheduler test: failed to connect to NATS: {err}");
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

    #[tokio::test]
    async fn test_check_health_dispatch() {
        let mut job = Job::new(
            crate::domain::JobId::from("job-1".to_string()),
            crate::domain::AppId::from("app-1".to_string()),
            "app1".to_string(),
            "img".to_string(),
            VmConfig::default(),
            crate::domain::UserId::from("user-1".to_string()),
            None,
        );
        job.schedule("host-1".to_string(), "vm-1".to_string());

        let job_repo = Arc::new(DummyJobRepo { job });
        let app_repo = Arc::new(DummyAppRepo);
        let worker_repo = Arc::new(DummyWorkerRepo);
        let agent_client = Arc::new(DummyAgentClient { healthy: true });

        // Use a lazy pool that doesn't connect for testing
        let _pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();

        let Some(nats_client) = connect_nats_or_skip().await else {
            return;
        };
        let service = AppService::new(
            job_repo,
            app_repo,
            worker_repo,
            agent_client,
            nats_client,
            _pool,
            test_runtime(),
        );

        let res = service.check_health("job-1", "user-1").await.unwrap();
        assert!(res);
    }

    fn paused_job() -> Job {
        let mut job = Job::new(
            crate::domain::JobId::from("job-1".to_string()),
            crate::domain::AppId::from("app-1".to_string()),
            "app1".to_string(),
            "img".to_string(),
            VmConfig::default(),
            crate::domain::UserId::from("user-1".to_string()),
            None,
        );
        job.schedule("host-1".to_string(), "vm-1".to_string());
        job.status = JobStatus::Running;
        job
    }

    #[tokio::test]
    async fn test_pause_app_success_updates_status_without_stop() {
        let Some(nats_client) = connect_nats_or_skip().await else {
            return;
        };

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
        let app_repo = Arc::new(DummyAppRepo);
        let job_repo = Arc::new(job_repo);
        let agent_client = Arc::new(agent_client);
        let _pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();
        let service = AppService::new(
            job_repo,
            app_repo,
            worker_repo,
            agent_client,
            nats_client,
            _pool,
            test_runtime(),
        );

        service.pause_app("job-1", "user-1").await.unwrap();
    }

    #[tokio::test]
    async fn test_pause_app_fallback_stops_vm_on_pause_failure() {
        let Some(nats_client) = connect_nats_or_skip().await else {
            return;
        };

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
        let app_repo = Arc::new(DummyAppRepo);
        let job_repo = Arc::new(job_repo);
        let agent_client = Arc::new(agent_client);
        let _pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();
        let service = AppService::new(
            job_repo,
            app_repo,
            worker_repo,
            agent_client,
            nats_client,
            _pool,
            test_runtime(),
        );

        service.pause_app("job-1", "user-1").await.unwrap();
    }

    #[tokio::test]
    async fn test_system_pause_bypasses_recent_traffic_guard() {
        let Some(nats_client) = connect_nats_or_skip().await else {
            return;
        };

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

        let app_repo = MockAppRepository::new();

        let worker_repo = Arc::new(MockWorkerRepository::new());
        let job_repo = Arc::new(job_repo);
        let app_repo = Arc::new(app_repo);
        let agent_client = Arc::new(agent_client);
        let _pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();
        let service = AppService::new(
            job_repo,
            app_repo,
            worker_repo,
            agent_client,
            nats_client,
            _pool,
            test_runtime(),
        );

        service.pause_app("job-1", "system").await.unwrap();
    }

    #[tokio::test]
    async fn test_delete_all_by_app_treats_missing_vm_as_success() {
        let Some(nats_client) = connect_nats_or_skip().await else {
            return;
        };

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
        let app_repo = Arc::new(DummyAppRepo);
        let job_repo = Arc::new(job_repo);
        let agent_client = Arc::new(agent_client);
        let _pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();
        let service = AppService::new(
            job_repo,
            app_repo,
            worker_repo,
            agent_client,
            nats_client,
            _pool,
            test_runtime(),
        );

        service.delete_all_by_app("app-1", "user-1").await.unwrap();
    }

    #[tokio::test]
    async fn test_delete_all_by_app_returns_error_when_vm_delete_fails() {
        let Some(nats_client) = connect_nats_or_skip().await else {
            return;
        };

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
        let app_repo = Arc::new(DummyAppRepo);
        let job_repo = Arc::new(job_repo);
        let agent_client = Arc::new(agent_client);
        let _pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();
        let service = AppService::new(
            job_repo,
            app_repo,
            worker_repo,
            agent_client,
            nats_client,
            _pool,
            test_runtime(),
        );

        let err = service
            .delete_all_by_app("app-1", "user-1")
            .await
            .expect_err("cleanup should fail");

        assert!(matches!(err, DomainError::Infrastructure(_)));
    }

    #[tokio::test]
    async fn test_resume_app_invalidates_missing_host_instead_of_calling_agent() {
        let Some(nats_client) = connect_nats_or_skip().await else {
            return;
        };

        let mut job = paused_job();
        job.status = JobStatus::Paused;

        let mut job_repo = MockJobRepository::new();
        job_repo.expect_get_job().with(eq("job-1")).returning({
            let job = job.clone();
            move |_| Ok(Some(job.clone()))
        });
        job_repo
            .expect_fail_job()
            .with(
                eq("job-1"),
                mockall::predicate::function(|msg: &String| msg == "Host host-1 no longer exists"),
                mockall::predicate::function(|ts: &i64| *ts > 0),
            )
            .times(1)
            .returning(|_, _, _| Ok(()));

        let mut worker_repo = MockWorkerRepository::new();
        worker_repo
            .expect_get_worker()
            .with(eq("host-1"))
            .times(1)
            .returning(|_| Ok(None));

        let mut agent_client = MockAgentClient::new();
        agent_client.expect_resume_vm().times(0);

        let app_repo = Arc::new(DummyAppRepo);
        let job_repo = Arc::new(job_repo);
        let worker_repo = Arc::new(worker_repo);
        let agent_client = Arc::new(agent_client);
        let _pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();
        let service = AppService::new(
            job_repo,
            app_repo,
            worker_repo,
            agent_client,
            nats_client,
            _pool,
            test_runtime(),
        );

        let resumed = service.resume_app("job-1", "user-1").await.unwrap();

        assert!(!resumed);
    }

    #[tokio::test]
    async fn test_resume_app_rolls_agent_back_when_persisting_running_fails() {
        let Some(nats_client) = connect_nats_or_skip().await else {
            return;
        };

        let mut job = paused_job();
        job.status = JobStatus::Paused;

        let mut job_repo = MockJobRepository::new();
        job_repo.expect_get_job().with(eq("job-1")).returning({
            let job = job.clone();
            move |_| Ok(Some(job.clone()))
        });
        job_repo
            .expect_update_job_status()
            .with(eq("job-1"), eq(JobStatus::Running))
            .times(1)
            .returning(|_, _| Err(DomainError::Infrastructure("db down".to_string())));

        let mut worker_repo = MockWorkerRepository::new();
        worker_repo
            .expect_get_worker()
            .with(eq("host-1"))
            .times(1)
            .returning(|_| {
                Ok(Some(crate::domain::worker::Worker {
                    host_id: crate::domain::HostId::from("host-1".to_string()),
                    hostname: "host1".to_string(),
                    advertise_address: "1.1.1.1".to_string(),
                    wireguard_pubkey: None,
                    wireguard_ip: None,
                    wireguard_port: None,
                    metrics: None,
                    registered_at: 0,
                    last_heartbeat: chrono::Utc::now().timestamp(),
                    status: crate::domain::WorkerStatus::Online,
                    supported_hypervisors: vec![],
                }))
            });

        let mut agent_client = MockAgentClient::new();
        agent_client
            .expect_resume_vm()
            .with(eq("host-1"), eq("vm-1"))
            .times(1)
            .returning(|_, _| Ok(()));
        agent_client
            .expect_pause_vm()
            .with(eq("host-1"), eq("vm-1"))
            .times(1)
            .returning(|_, _| Ok(()));
        agent_client.expect_stop_vm().times(0);

        let app_repo = Arc::new(DummyAppRepo);
        let job_repo = Arc::new(job_repo);
        let worker_repo = Arc::new(worker_repo);
        let agent_client = Arc::new(agent_client);
        let _pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();
        let service = AppService::new(
            job_repo,
            app_repo,
            worker_repo,
            agent_client,
            nats_client,
            _pool,
            test_runtime(),
        );

        let err = service.resume_app("job-1", "user-1").await.unwrap_err();
        assert!(matches!(err, DomainError::Infrastructure(_)));
    }

    #[tokio::test]
    async fn test_deploy_app_rolls_back_job_when_start_job_persist_fails() {
        let Some(nats_client) = connect_nats_or_skip().await else {
            return;
        };

        let worker = crate::domain::worker::Worker {
            host_id: crate::domain::HostId::from("host-1".to_string()),
            hostname: "host1".to_string(),
            advertise_address: "1.1.1.1".to_string(),
            wireguard_pubkey: None,
            wireguard_ip: None,
            wireguard_port: None,
            metrics: Some(crate::domain::HostMetrics {
                cpu_usage: 10.0,
                ram_used_bytes: 512 * 1024 * 1024,
                ram_total_bytes: 8 * 1024 * 1024 * 1024,
                disk_used_bytes: 1024 * 1024 * 1024,
                disk_total_bytes: 16 * 1024 * 1024 * 1024,
                apps_count: 0,
                load_avg_1: 0.1,
                load_avg_5: 0.1,
                load_avg_15: 0.1,
                timestamp: chrono::Utc::now().timestamp(),
                vms: std::collections::HashMap::new(),
            }),
            registered_at: 0,
            last_heartbeat: chrono::Utc::now().timestamp(),
            status: crate::domain::WorkerStatus::Online,
            supported_hypervisors: vec![],
        };

        let mut job_repo = MockJobRepository::new();
        job_repo.expect_list_jobs().returning(|_, _, _| Ok(vec![]));
        job_repo.expect_add_job().times(1).returning(|_| Ok(()));
        job_repo
            .expect_start_job()
            .times(1)
            .returning(|_, _| Err(DomainError::Infrastructure("db down".to_string())));
        job_repo
            .expect_remove_job()
            .withf(|job_id| !job_id.is_empty())
            .times(1)
            .returning(|_| Ok(()));

        let mut worker_repo = MockWorkerRepository::new();
        worker_repo
            .expect_get_available_workers()
            .with(eq(30))
            .times(1)
            .returning({
                let worker = worker.clone();
                move |_| Ok(vec![worker.clone()])
            });

        let mut agent_client = MockAgentClient::new();
        agent_client
            .expect_start_vm()
            .times(1)
            .returning(|_, _, _, _, _| Ok(()));
        agent_client
            .expect_delete_vm()
            .with(
                eq("host-1"),
                mockall::predicate::function(|vm_id: &str| !vm_id.is_empty()),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        let app_repo = Arc::new(DummyAppRepo);
        let job_repo = Arc::new(job_repo);
        let worker_repo = Arc::new(worker_repo);
        let agent_client = Arc::new(agent_client);
        let _pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();
        let service = AppService::new(
            job_repo,
            app_repo,
            worker_repo,
            agent_client,
            nats_client,
            _pool,
            test_runtime(),
        );

        let err = service
            .deployment
            .deploy_app(crate::application::deployment::DeployAppParams {
                app_id: "app-1".to_string(),
                app_name: "app".to_string(),
                image: "image:latest".to_string(),
                user_id: "user-1".to_string(),
                deployment_id: "dep-1".to_string(),
                vpc_ipv6_prefix: "fd00::".to_string(),
                config: VmConfig::default(),
                strategy: crate::domain::SchedulingStrategy::LeastLoaded,
            })
            .await
            .unwrap_err();

        assert!(matches!(err, DomainError::Infrastructure(_)));
    }

    #[tokio::test]
    async fn test_delete_app_marks_job_stopped_when_remove_fails() {
        let Some(nats_client) = connect_nats_or_skip().await else {
            return;
        };

        let job = paused_job();
        let mut job_repo = MockJobRepository::new();
        job_repo.expect_get_job().with(eq("job-1")).returning({
            let job = job.clone();
            move |_| Ok(Some(job.clone()))
        });
        job_repo
            .expect_remove_job()
            .with(eq("job-1"))
            .times(1)
            .returning(|_| Err(DomainError::Infrastructure("db down".to_string())));
        job_repo
            .expect_update_job_status()
            .with(eq("job-1"), eq(JobStatus::Stopped))
            .times(1)
            .returning(|_, _| Ok(()));

        let mut agent_client = MockAgentClient::new();
        agent_client
            .expect_delete_vm()
            .with(eq("host-1"), eq("vm-1"))
            .times(1)
            .returning(|_, _| Ok(()));

        let app_repo = Arc::new(DummyAppRepo);
        let job_repo = Arc::new(job_repo);
        let worker_repo = Arc::new(MockWorkerRepository::new());
        let agent_client = Arc::new(agent_client);
        let _pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();
        let service = AppService::new(
            job_repo,
            app_repo,
            worker_repo,
            agent_client,
            nats_client,
            _pool,
            test_runtime(),
        );

        let err = service.delete_app("job-1", "user-1").await.unwrap_err();
        assert!(matches!(err, DomainError::Infrastructure(_)));
    }

    #[test]
    fn autoscaling_target_scales_down_when_usage_is_below_hysteresis_band() {
        let app = AppConfig {
            id: crate::domain::AppId::from("app-1".to_string()),
            user_id: crate::domain::UserId::from("user-1".to_string()),
            vpc_ipv6_prefix: "fd00::".to_string(),
            desired_replicas: 3,
            min_replicas: 1,
            max_replicas: 3,
            autoscaling_enabled: true,
            cpu_threshold: 80.0,
            mem_threshold: 80.0,
            hostname: "app.example.com".to_string(),
            last_router_traffic_at: 0,
            last_scaled_to_zero_at: 0,
            restore_retry_after_at: 0,
        };

        let target = autoscale_next_replicas(&app, 3, 30.0, 25.0);

        assert_eq!(target, 2);
    }

    #[test]
    fn autoscaling_target_holds_size_inside_hysteresis_band() {
        let app = AppConfig {
            id: crate::domain::AppId::from("app-1".to_string()),
            user_id: crate::domain::UserId::from("user-1".to_string()),
            vpc_ipv6_prefix: "fd00::".to_string(),
            desired_replicas: 3,
            min_replicas: 1,
            max_replicas: 3,
            autoscaling_enabled: true,
            cpu_threshold: 80.0,
            mem_threshold: 80.0,
            hostname: "app.example.com".to_string(),
            last_router_traffic_at: 0,
            last_scaled_to_zero_at: 0,
            restore_retry_after_at: 0,
        };

        let target = autoscale_next_replicas(&app, 3, 70.0, 70.0);

        assert_eq!(target, 3);
    }

    #[test]
    fn autoscaling_target_keeps_current_size_when_usage_matches_threshold() {
        let app = AppConfig {
            id: crate::domain::AppId::from("app-1".to_string()),
            user_id: crate::domain::UserId::from("user-1".to_string()),
            vpc_ipv6_prefix: "fd00::".to_string(),
            desired_replicas: 2,
            min_replicas: 1,
            max_replicas: 3,
            autoscaling_enabled: true,
            cpu_threshold: 80.0,
            mem_threshold: 80.0,
            hostname: "app.example.com".to_string(),
            last_router_traffic_at: 0,
            last_scaled_to_zero_at: 0,
            restore_retry_after_at: 0,
        };

        let target = autoscale_next_replicas(&app, 2, 80.0, 80.0);

        assert_eq!(target, 2);
    }
}
