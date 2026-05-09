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
        let status_str = job.status.as_str();

        sqlx::query(
            r#"
            INSERT INTO jobs (
                job_id, app_id, app_name, image, user_id, status, host_id, vm_id,
                vcpus, memory_mib, disk_mib, port, env_vars, ip_address, gateway,
                mac_address, netmask, created_at, deployment_id, health_check_path,
                ipv6_address, ipv6_gateway
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22)
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
        .bind(&job.config.health_check_path)
        .bind(&job.config.ipv6_address)
        .bind(&job.config.ipv6_gateway)
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

    async fn update_job_ip(
        &self,
        job_id: &str,
        ip: &str,
        gateway: &str,
        mac: &str,
        netmask: &str,
    ) -> DomainResult<()> {
        sqlx::query(
            "UPDATE jobs SET ip_address = $1, gateway = $2, mac_address = $3, netmask = $4 WHERE job_id = $5"
        )
        .bind(ip)
        .bind(gateway)
        .bind(mac)
        .bind(netmask)
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
        let mut query = "SELECT * FROM jobs WHERE 1=1".to_string();
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
            INSERT INTO workers (id, hostname, ip_address, bridge_ip, wireguard_pubkey, wireguard_ip, wireguard_port, last_heartbeat, registered_at, status)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'Online')
            ON CONFLICT (id) DO UPDATE SET
                hostname = EXCLUDED.hostname,
                ip_address = EXCLUDED.ip_address,
                bridge_ip = EXCLUDED.bridge_ip,
                wireguard_pubkey = EXCLUDED.wireguard_pubkey,
                wireguard_ip = EXCLUDED.wireguard_ip,
                wireguard_port = EXCLUDED.wireguard_port,
                last_heartbeat = EXCLUDED.last_heartbeat,
                status = 'Online'
            "#,
        )
        .bind(&worker.host_id)
        .bind(&worker.hostname)
        .bind(&worker.ip_address)
        .bind(&worker.bridge_ip)
        .bind(&worker.wireguard_pubkey)
        .bind(&worker.wireguard_ip)
        .bind(worker.wireguard_port.unwrap_or(51820))
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
            "SELECT id, hostname, ip_address, bridge_ip, wireguard_pubkey, wireguard_ip, wireguard_port, metrics, registered_at, last_heartbeat FROM workers WHERE id = $1"
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
            "SELECT id, hostname, ip_address, bridge_ip, wireguard_pubkey, wireguard_ip, wireguard_port, metrics, registered_at, last_heartbeat FROM workers"
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
            SELECT id, hostname, ip_address, bridge_ip, wireguard_pubkey, wireguard_ip, wireguard_port, metrics, registered_at, last_heartbeat
            FROM workers
            WHERE metrics IS NOT NULL AND last_heartbeat > $1 AND status = 'Online'
            "#,
        )
        .bind(threshold)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(map_row_to_worker).collect())
    }
}

fn map_row_to_job(r: &sqlx::postgres::PgRow) -> Job {
    let status_str: String = r.get("status");
    let status = match status_str.as_str() {
        "pending" => JobStatus::Pending,
        "scheduled" => JobStatus::Scheduled,
        "running" => JobStatus::Running,
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
            ip_address: r.get("ip_address"),
            gateway: r.get("gateway"),
            mac_address: r.get("mac_address"),
            netmask: r.get("netmask"),
            ipv6_address: r.get("ipv6_address"),
            ipv6_gateway: r.get("ipv6_gateway"),
            volumes: vec![], // TODO: Volumes
            health_check_path: r.get("health_check_path"),
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
        bridge_ip: r.get::<String, _>("bridge_ip").clone(),
        wireguard_pubkey: r.get("wireguard_pubkey"),
        wireguard_ip: r.get("wireguard_ip"),
        wireguard_port: r.try_get("wireguard_port").ok(),
        metrics,
        registered_at: r.get("registered_at"),
        last_heartbeat: r.get("last_heartbeat"),
    }
}
