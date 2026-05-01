pub mod ipam;

use crate::domain::{
    DomainResult, HostMetrics, Job, JobRepository, JobStatus, VmConfig, Worker, WorkerRepository,
};
use async_trait::async_trait;
use sqlx::{PgPool, Row};
use std::collections::HashMap;

pub struct PgJobRepository {
    pool: PgPool,
}

impl PgJobRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl JobRepository for PgJobRepository {
    async fn add_job(&self, job: Job) -> DomainResult<()> {
        let env_json = serde_json::to_value(&job.config.env).unwrap_or_default();
        let status_str = serde_json::to_string(&job.status)
            .unwrap_or_else(|_| "\"pending\"".to_string())
            .trim_matches('"')
            .to_string();

        sqlx::query(
            r#"
            INSERT INTO jobs (
                job_id, app_id, app_name, image, user_id, status, host_id, vm_id,
                vcpus, memory_mib, disk_mib, port, env_vars, ip_address, gateway,
                mac_address, netmask, created_at, deployment_id
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
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
        .bind(&job.config.ip_address)
        .bind(&job.config.gateway)
        .bind(&job.config.mac_address)
        .bind(&job.config.netmask)
        .bind(job.created_at)
        .bind(&job.deployment_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_job(&self, job_id: &str) -> DomainResult<Option<Job>> {
        let row = sqlx::query("SELECT * FROM jobs WHERE job_id = $1")
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
        let status_str = serde_json::to_string(&status)
            .unwrap_or_else(|_| "\"pending\"".to_string())
            .trim_matches('"')
            .to_string();

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

    async fn list_jobs(
        &self,
        user_id: Option<&str>,
        _status: Option<JobStatus>,
    ) -> DomainResult<Vec<Job>> {
        let rows = if let Some(uid) = user_id {
            sqlx::query("SELECT * FROM jobs WHERE user_id = $1")
                .bind(uid)
                .fetch_all(&self.pool)
                .await?
        } else {
            sqlx::query("SELECT * FROM jobs")
                .fetch_all(&self.pool)
                .await?
        };

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

impl PgWorkerRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl WorkerRepository for PgWorkerRepository {
    async fn register(&self, worker: Worker) -> DomainResult<()> {
        let now = chrono::Utc::now().timestamp();

        // Remove any stale worker with the same hostname but different host_id
        sqlx::query("DELETE FROM workers WHERE hostname = $1 AND id != $2")
            .bind(&worker.hostname)
            .bind(&worker.host_id)
            .execute(&self.pool)
            .await?;

        // Upsert the worker
        sqlx::query(
            r#"
            INSERT INTO workers (id, hostname, ip_address, agent_port, bridge_ip, last_heartbeat, registered_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (id) DO UPDATE SET
                hostname = EXCLUDED.hostname,
                ip_address = EXCLUDED.ip_address,
                agent_port = EXCLUDED.agent_port,
                bridge_ip = EXCLUDED.bridge_ip,
                last_heartbeat = EXCLUDED.last_heartbeat
            "#
        )
        .bind(&worker.host_id)
        .bind(&worker.hostname)
        .bind(&worker.ip_address)
        .bind(worker.agent_port as i32)
        .bind(&worker.bridge_ip)
        .bind(now)
        .bind(now)
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
        let row = sqlx::query(
            "SELECT id, hostname, ip_address, agent_port, bridge_ip, metrics, registered_at, last_heartbeat FROM workers WHERE id = $1"
        )
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
        let rows = sqlx::query(
            "SELECT id, hostname, ip_address, agent_port, bridge_ip, metrics, registered_at, last_heartbeat FROM workers"
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(map_row_to_worker).collect())
    }

    async fn get_available_workers(&self, threshold_secs: i64) -> DomainResult<Vec<Worker>> {
        let now = chrono::Utc::now().timestamp();
        let threshold = now - threshold_secs;

        let rows = sqlx::query(
            r#"
            SELECT id, hostname, ip_address, agent_port, bridge_ip, metrics, registered_at, last_heartbeat
            FROM workers
            WHERE metrics IS NOT NULL AND last_heartbeat > $1 AND status = 'Online'
            "#
        )
        .bind(threshold)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(map_row_to_worker).collect())
    }
}

fn map_row_to_job(r: &sqlx::postgres::PgRow) -> Job {
    let status_str: String = r.get("status");
    let status: JobStatus =
        serde_json::from_str(&format!("\"{}\"", status_str)).unwrap_or_default();
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
            ip_address: r.get("ip_address"),
            gateway: r.get("gateway"),
            mac_address: r.get("mac_address"),
            netmask: r.get("netmask"),
            volumes: vec![], // TODO: Volumes
        },
    }
}

fn map_row_to_worker(r: &sqlx::postgres::PgRow) -> Worker {
    let metrics_val: Option<serde_json::Value> = r.try_get("metrics").ok();
    let metrics: Option<HostMetrics> = metrics_val.and_then(|m| serde_json::from_value(m).ok());
    Worker {
        host_id: r.get("id"),
        hostname: r.get("hostname"),
        ip_address: r.get("ip_address"),
        agent_port: r.get::<i32, _>("agent_port") as u16,
        bridge_ip: r.get::<String, _>("bridge_ip").clone(),
        metrics,
        registered_at: r.get("registered_at"),
        last_heartbeat: r.get("last_heartbeat"),
    }
}
