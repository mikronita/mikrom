use crate::domain::{
    AppConfig, AppRepository, DomainResult, HostMetrics, Job, JobRepository, JobStatus, VmConfig,
    Worker, WorkerRepository,
};
use async_trait::async_trait;
use sqlx::{PgPool, Row};
use std::collections::HashMap;

pub struct PgJobRepository {
    pool: PgPool,
}

const JOB_COLUMNS: &str = "job_id, app_id, app_name, image, user_id, status, host_id, vm_id, vcpus, memory_mib, disk_mib, port, env_vars, created_at, deployment_id, health_check_path, ipv6_address, ipv6_gateway, scheduled_at, started_at, stopped_at, error_message";

impl PgJobRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl JobRepository for PgJobRepository {
    async fn add_job(&self, job: Job) -> DomainResult<()> {
        let env_json = serde_json::to_value(&job.config.env).unwrap_or_default();
        let status_str = job.status.as_str();

        sqlx::query(
            r#"
            INSERT INTO jobs (
                job_id, app_id, app_name, image, user_id, status, host_id, vm_id,
                vcpus, memory_mib, disk_mib, port, env_vars, created_at, deployment_id, health_check_path,
                ipv6_address, ipv6_gateway
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
            "#
        )
        .bind(&job.job_id)
        .bind(&job.app_id)
        .bind(&job.app_name)
        .bind(&job.image)
        .bind(&job.user_id)
        .bind(status_str)
        .bind(&job.host_id)
        .bind(&job.vm_id)
        .bind(job.config.vcpus as i32)
        .bind(job.config.memory_mib as i64)
        .bind(job.config.disk_mib as i64)
        .bind(job.config.port as i32)
        .bind(env_json)
        .bind(job.created_at)
        .bind(&job.deployment_id)
        .bind(&job.config.health_check_path)
        .bind(&job.config.ipv6_address)
        .bind(&job.config.ipv6_gateway)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_job(&self, job_id: &str) -> DomainResult<Option<Job>> {
        let query = format!("SELECT {} FROM jobs WHERE job_id = $1", JOB_COLUMNS);
        let row = sqlx::query(&query)
            .bind(job_id)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(r) = row {
            Ok(Some(map_row_to_job(&r)))
        } else {
            Ok(None)
        }
    }

    async fn update_job_status(&self, job_id: &str, status: JobStatus) -> DomainResult<()> {
        let status_str = status.as_str();

        sqlx::query("UPDATE jobs SET status = $1 WHERE job_id = $2")
            .bind(status_str)
            .bind(job_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn start_job(&self, job_id: &str, timestamp: i64) -> DomainResult<()> {
        sqlx::query("UPDATE jobs SET status = 'running', started_at = $1 WHERE job_id = $2")
            .bind(timestamp)
            .bind(job_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn fail_job(&self, job_id: &str, message: String, timestamp: i64) -> DomainResult<()> {
        sqlx::query(
            "UPDATE jobs SET status = 'failed', error_message = $1, stopped_at = $2 WHERE job_id = $3"
        )
        .bind(message)
        .bind(timestamp)
        .bind(job_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn cancel_job(&self, job_id: &str, timestamp: i64) -> DomainResult<()> {
        sqlx::query("UPDATE jobs SET status = 'cancelled', stopped_at = $1 WHERE job_id = $2")
            .bind(timestamp)
            .bind(job_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn remove_job(&self, job_id: &str) -> DomainResult<()> {
        sqlx::query("DELETE FROM jobs WHERE job_id = $1")
            .bind(job_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn remove_jobs_by_app(&self, app_id: &str) -> DomainResult<()> {
        sqlx::query("DELETE FROM jobs WHERE app_id = $1")
            .bind(app_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_jobs<'a>(
        &self,
        user_id: Option<&'a str>,
        app_id: Option<&'a str>,
        status: Option<JobStatus>,
    ) -> DomainResult<Vec<Job>> {
        let mut query = format!("SELECT {} FROM jobs WHERE 1=1", JOB_COLUMNS);
        let mut params_count = 1;

        if user_id.is_some() {
            query.push_str(&format!(" AND user_id = ${}", params_count));
            params_count += 1;
        }
        if app_id.is_some() {
            query.push_str(&format!(" AND app_id = ${}", params_count));
            params_count += 1;
        }
        if status.is_some() {
            query.push_str(&format!(" AND status = ${}", params_count));
        }

        let mut q = sqlx::query(&query);
        if let Some(uid) = user_id {
            q = q.bind(uid);
        }
        if let Some(aid) = app_id {
            q = q.bind(aid);
        }
        if let Some(st) = status {
            q = q.bind(st.as_str());
        }

        let rows = q.fetch_all(&self.pool).await?;
        Ok(rows.iter().map(map_row_to_job).collect())
    }

    async fn find_job_by_vm_id(&self, vm_id: &str) -> DomainResult<Option<Job>> {
        let row = sqlx::query("SELECT job_id FROM jobs WHERE vm_id = $1")
            .bind(vm_id)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(r) = row {
            let job_id: String = r.get("job_id");
            self.get_job(&job_id).await
        } else {
            Ok(None)
        }
    }
}

pub struct PgWorkerRepository {
    pool: PgPool,
}

const WORKER_COLUMNS: &str = "id, hostname, advertise_address, wireguard_pubkey, wireguard_ip, wireguard_port, metrics, registered_at, last_heartbeat, status, supported_hypervisors";

impl PgWorkerRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl WorkerRepository for PgWorkerRepository {
    async fn register(&self, worker: Worker) -> DomainResult<()> {
        let now = chrono::Utc::now().timestamp();
        let hvs: Vec<i32> = worker.supported_hypervisors.iter().map(|&h| h as i32).collect();

        // Keep the worker record hot with a single upsert. We avoid a pre-delete because it
        // amplifies write contention on the workers table under heartbeat bursts.
        sqlx::query(
            r#"
            INSERT INTO workers (id, hostname, advertise_address, wireguard_pubkey, wireguard_ip, wireguard_port, last_heartbeat, registered_at, status, supported_hypervisors)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (id) DO UPDATE SET
                hostname = EXCLUDED.hostname,
                advertise_address = EXCLUDED.advertise_address,
                wireguard_pubkey = EXCLUDED.wireguard_pubkey,
                wireguard_ip = EXCLUDED.wireguard_ip,
                wireguard_port = EXCLUDED.wireguard_port,
                last_heartbeat = EXCLUDED.last_heartbeat,
                status = EXCLUDED.status,
                supported_hypervisors = EXCLUDED.supported_hypervisors
            "#,
        )
        .bind(&worker.host_id)
        .bind(&worker.hostname)
        .bind(&worker.advertise_address)
        .bind(&worker.wireguard_pubkey)
        .bind(&worker.wireguard_ip)
        .bind(worker.wireguard_port.unwrap_or(51820))
        .bind(now)
        .bind(now)
        .bind(worker.status.as_str())
        .bind(&hvs)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn unregister(&self, host_id: &str) -> DomainResult<()> {
        sqlx::query("DELETE FROM workers WHERE id = $1")
            .bind(host_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn update_metrics(&self, host_id: &str, metrics: HostMetrics) -> DomainResult<()> {
        let now = chrono::Utc::now().timestamp();
        let metrics_json = serde_json::to_value(metrics).unwrap_or_default();

        sqlx::query(
            r#"
            UPDATE workers
            SET metrics = $1, last_heartbeat = $2, status = 'Online'
            WHERE id = $3
            "#,
        )
        .bind(metrics_json)
        .bind(now)
        .bind(host_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_worker(&self, host_id: &str) -> DomainResult<Option<Worker>> {
        let query = format!("SELECT {} FROM workers WHERE id = $1", WORKER_COLUMNS);
        let row = sqlx::query(&query)
            .bind(host_id)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(r) = row {
            Ok(Some(map_row_to_worker(&r)))
        } else {
            Ok(None)
        }
    }

    async fn list_workers(&self) -> DomainResult<Vec<Worker>> {
        let query = format!("SELECT {} FROM workers ORDER BY id", WORKER_COLUMNS);
        let rows = sqlx::query(&query).fetch_all(&self.pool).await?;

        Ok(rows.iter().map(map_row_to_worker).collect())
    }

    async fn get_available_workers(&self, threshold_secs: i64) -> DomainResult<Vec<Worker>> {
        let now = chrono::Utc::now().timestamp();
        let threshold = now - threshold_secs;

        let query = format!(
            "SELECT {} FROM workers WHERE metrics IS NOT NULL AND last_heartbeat > $1 AND status = 'Online' ORDER BY id",
            WORKER_COLUMNS
        );
        let rows = sqlx::query(&query)
            .bind(threshold)
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.iter().map(map_row_to_worker).collect())
    }

    async fn mark_stale_workers_offline(&self, threshold_secs: i64) -> DomainResult<u64> {
        let now = chrono::Utc::now().timestamp();
        let threshold = now - threshold_secs;

        let result = sqlx::query(
            "UPDATE workers SET status = 'Offline' WHERE last_heartbeat < $1 AND status = 'Online'",
        )
        .bind(threshold)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }
}

fn map_row_to_job(r: &sqlx::postgres::PgRow) -> Job {
    let status_str: String = r.get("status");
    let status = match status_str.as_str() {
        "pending" => JobStatus::Pending,
        "scheduled" => JobStatus::Scheduled,
        "running" => JobStatus::Running,
        "paused" => JobStatus::Paused,
        "stopped" => JobStatus::Stopped,
        "failed" => JobStatus::Failed,
        "cancelled" => JobStatus::Cancelled,
        _ => JobStatus::default(),
    };
    let env_vars: serde_json::Value = r.get("env_vars");
    let env: HashMap<String, String> = serde_json::from_value(env_vars).unwrap_or_default();

    Job {
        job_id: r.get("job_id"),
        app_id: r.get("app_id"),
        app_name: r.get("app_name"),
        image: r.get("image"),
        user_id: r.get("user_id"),
        status,
        host_id: r.get("host_id"),
        vm_id: r.get("vm_id"),
        scheduled_at: r.get("scheduled_at"),
        started_at: r.get("started_at"),
        stopped_at: r.get("stopped_at"),
        error_message: r.get("error_message"),
        created_at: r.get("created_at"),
        deployment_id: r.get("deployment_id"),
        config: VmConfig {
            vcpus: r.get::<i32, _>("vcpus") as u32,
            memory_mib: r.get::<i64, _>("memory_mib") as u64,
            disk_mib: r.get::<i64, _>("disk_mib") as u64,
            port: r.get::<i32, _>("port") as u32,
            env,
            ipv6_address: r.get("ipv6_address"),
            ipv6_gateway: r.get("ipv6_gateway"),
            volumes: vec![], // TODO: Volumes
            health_check_path: r.get("health_check_path"),
            hypervisor: Default::default(),
        },
    }
}

fn map_row_to_worker(r: &sqlx::postgres::PgRow) -> Worker {
    let metrics_val: Option<serde_json::Value> = r.try_get("metrics").ok();
    let metrics: Option<HostMetrics> = metrics_val.and_then(|m| serde_json::from_value(m).ok());
    let status_str: String = r.get("status");
    let status = match status_str.as_str() {
        "Online" => crate::domain::WorkerStatus::Online,
        _ => crate::domain::WorkerStatus::Offline,
    };
    let hvs_raw: Vec<i32> = r.try_get("supported_hypervisors").unwrap_or_default();
    let supported_hypervisors = hvs_raw
        .into_iter()
        .filter_map(crate::domain::job::HypervisorType::from_i32)
        .collect();

    Worker {
        host_id: r.get("id"),
        hostname: r.get("hostname"),
        advertise_address: r.get("advertise_address"),
        wireguard_pubkey: r.get("wireguard_pubkey"),
        wireguard_ip: r.get("wireguard_ip"),
        wireguard_port: r.try_get("wireguard_port").ok(),
        metrics,
        registered_at: r.get("registered_at"),
        last_heartbeat: r.get("last_heartbeat"),
        status,
        supported_hypervisors,
    }
}

pub struct PgAppRepository {
    pool: PgPool,
}

impl PgAppRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AppRepository for PgAppRepository {
    async fn update_app_config(&self, config: AppConfig) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO apps (
                id, user_id, vpc_ipv6_prefix, hostname, desired_replicas, min_replicas, max_replicas,
                autoscaling_enabled, cpu_threshold, mem_threshold, last_router_traffic_at,
                last_scaled_to_zero_at, restore_retry_after_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, NOW())
            ON CONFLICT (id) DO UPDATE SET
                vpc_ipv6_prefix = EXCLUDED.vpc_ipv6_prefix,
                hostname = EXCLUDED.hostname,
                desired_replicas = EXCLUDED.desired_replicas,
                min_replicas = EXCLUDED.min_replicas,
                max_replicas = EXCLUDED.max_replicas,
                autoscaling_enabled = EXCLUDED.autoscaling_enabled,
                cpu_threshold = EXCLUDED.cpu_threshold,
                mem_threshold = EXCLUDED.mem_threshold,
                last_router_traffic_at = EXCLUDED.last_router_traffic_at,
                last_scaled_to_zero_at = EXCLUDED.last_scaled_to_zero_at,
                restore_retry_after_at = EXCLUDED.restore_retry_after_at,
                updated_at = NOW()
            "#,
        )
        .bind(&config.id)
        .bind(&config.user_id)
        .bind(&config.vpc_ipv6_prefix)
        .bind(&config.hostname)
        .bind(config.desired_replicas as i32)
        .bind(config.min_replicas as i32)
        .bind(config.max_replicas as i32)
        .bind(config.autoscaling_enabled)
        .bind(config.cpu_threshold)
        .bind(config.mem_threshold)
        .bind(config.last_router_traffic_at)
        .bind(config.last_scaled_to_zero_at)
        .bind(config.restore_retry_after_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_app_config(&self, app_id: &str) -> anyhow::Result<Option<AppConfig>> {
        let row = sqlx::query(
            "SELECT id, user_id, vpc_ipv6_prefix, hostname, desired_replicas, min_replicas, max_replicas, autoscaling_enabled, cpu_threshold, mem_threshold, last_router_traffic_at, last_scaled_to_zero_at, restore_retry_after_at FROM apps WHERE id = $1"
        )
        .bind(app_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| map_row_to_app_config(&r)))
    }

    async fn get_app_config_by_hostname(
        &self,
        hostname: &str,
    ) -> anyhow::Result<Option<AppConfig>> {
        let row = sqlx::query(
            "SELECT id, user_id, vpc_ipv6_prefix, hostname, desired_replicas, min_replicas, max_replicas, autoscaling_enabled, cpu_threshold, mem_threshold, last_router_traffic_at, last_scaled_to_zero_at, restore_retry_after_at FROM apps WHERE hostname = $1"
        )
        .bind(hostname)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| map_row_to_app_config(&r)))
    }

    async fn list_all_apps(&self) -> anyhow::Result<Vec<AppConfig>> {
        let rows = sqlx::query(
            "SELECT id, user_id, vpc_ipv6_prefix, hostname, desired_replicas, min_replicas, max_replicas, autoscaling_enabled, cpu_threshold, mem_threshold, last_router_traffic_at, last_scaled_to_zero_at, restore_retry_after_at FROM apps"
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(map_row_to_app_config).collect())
    }

    async fn list_autoscaling_apps(&self) -> anyhow::Result<Vec<AppConfig>> {
        let rows = sqlx::query(
            "SELECT id, user_id, vpc_ipv6_prefix, hostname, desired_replicas, min_replicas, max_replicas, autoscaling_enabled, cpu_threshold, mem_threshold, last_router_traffic_at, last_scaled_to_zero_at, restore_retry_after_at FROM apps WHERE autoscaling_enabled = TRUE"
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(map_row_to_app_config).collect())
    }

    async fn remove_app_config(&self, app_id: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM apps WHERE id = $1")
            .bind(app_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn remove_app_and_jobs_by_app(&self, app_id: &str) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("DELETE FROM jobs WHERE app_id = $1")
            .bind(app_id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM apps WHERE id = $1")
            .bind(app_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }
}

fn map_row_to_app_config(r: &sqlx::postgres::PgRow) -> AppConfig {
    AppConfig {
        id: r.get("id"),
        user_id: r.get("user_id"),
        vpc_ipv6_prefix: r.get("vpc_ipv6_prefix"),
        hostname: r.get("hostname"),
        desired_replicas: r.get::<i32, _>("desired_replicas") as u32,
        min_replicas: r.get::<i32, _>("min_replicas") as u32,
        max_replicas: r.get::<i32, _>("max_replicas") as u32,
        autoscaling_enabled: r.get("autoscaling_enabled"),
        cpu_threshold: r.get("cpu_threshold"),
        mem_threshold: r.get("mem_threshold"),
        last_router_traffic_at: r.get("last_router_traffic_at"),
        last_scaled_to_zero_at: r.get("last_scaled_to_zero_at"),
        restore_retry_after_at: r.get("restore_retry_after_at"),
    }
}
