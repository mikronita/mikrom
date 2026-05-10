use crate::job::{Job, JobStatus, VmConfig};
use crate::worker_registry::{Worker, WorkerRegistry};
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use thiserror::Error;

pub mod ipam;

/// Errors that can occur during the scheduling process.
#[derive(Error, Debug)]
pub enum SchedulerError {
    /// No workers are currently registered in the cluster.
    #[error("No available workers")]
    NoWorkers,
    /// No registered worker has enough resources to satisfy the VM requirements.
    #[error("No worker can fit the VM requirements")]
    NoFit,
    /// The requested job ID was not found in the scheduler state.
    #[error("Job not found: {0}")]
    JobNotFound(String),
    /// The IP address pool for the target worker is exhausted.
    #[error("IP address pool exhausted")]
    IpPoolExhausted,
    /// Database error
    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),
}

const MAX_APPS_PER_HOST: u32 = 10;

/// Strategies for selecting a worker to host a new VM.
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub enum SchedulingStrategy {
    /// Spreads the load across all available workers (default).
    #[default]
    LeastLoaded,
    /// Fills workers sequentially to minimize the number of active nodes.
    BinPacking,
}

/// The core component responsible for matching VM requests to available workers.
///
/// It maintains the state of all active jobs and orchestrates the worker registry.
#[derive(Clone)]
pub struct AppScheduler {
    pub pool: PgPool,
    /// Registry of all active workers and their resource availability.
    pub worker_registry: WorkerRegistry,
    /// The strategy used for placing new workloads.
    pub strategy: SchedulingStrategy,
    /// NATS client for cluster-wide updates.
    pub nats_client: Option<async_nats::Client>,
    /// Broadcast channel for job updates (local to this instance, keeping for now).
    pub job_updates: tokio::sync::broadcast::Sender<Job>,
}

impl AppScheduler {
    /// Creates a new scheduler with the provided worker registry and pool.
    #[must_use]
    pub fn new(pool: PgPool, worker_registry: WorkerRegistry) -> Self {
        let (job_updates, _) = tokio::sync::broadcast::channel(1024);
        Self {
            pool,
            worker_registry,
            strategy: SchedulingStrategy::default(),
            nats_client: None,
            job_updates,
        }
    }

    pub fn set_nats_client(&mut self, client: async_nats::Client) {
        self.nats_client = Some(client);
    }

    async fn notify_job_update(&self, job: Job) {
        let _ = self.job_updates.send(job.clone());

        if let Some(ref nats) = self.nats_client {
            use mikrom_proto::scheduler::AppInfo;
            use prost::Message;

            let (cpu_usage, ram_used_bytes, tx_bytes, rx_bytes) = if let Some(host_id) = &job.host_id {
                if let Ok(Some(metrics)) = self.worker_registry.get_metrics(host_id).await {
                    if let Some(vm_id) = &job.vm_id {
                        if let Some(vm_m) = metrics.vms.get(vm_id) {
                            (
                                vm_m.cpu_usage,
                                vm_m.ram_used_bytes,
                                vm_m.tx_bytes,
                                vm_m.rx_bytes,
                            )
                        } else {
                            (0.0, 0, 0, 0)
                        }
                    } else {
                        (0.0, 0, 0, 0)
                    }
                } else {
                    (0.0, 0, 0, 0)
                }
            } else {
                (0.0, 0, 0, 0)
            };

            let app_info = AppInfo {
                job_id: job.job_id,
                app_id: job.app_id,
                app_name: job.app_name,
                image: job.image,
                status: job.status as i32,
                host_id: job.host_id.unwrap_or_default(),
                vm_id: job.vm_id.unwrap_or_default(),
                cpu_usage,
                ram_used_bytes,
                user_id: job.user_id,
                deployment_id: job.deployment_id.unwrap_or_default(),
                ipv6_address: job.config.ipv6_address.unwrap_or_default(),
                tx_bytes,
                rx_bytes,
            };

            let mut buf = Vec::new();
            if app_info.encode(&mut buf).is_ok() {
                let _ = nats
                    .publish("mikrom.scheduler.job_updates", buf.into())
                    .await;
            }
        }
    }

    #[must_use]
    pub fn with_strategy(mut self, strategy: SchedulingStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    #[must_use]
    pub fn worker_registry(&self) -> &WorkerRegistry {
        &self.worker_registry
    }

    pub async fn add_job(&self, job: Job) -> Result<(), SchedulerError> {
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

        self.notify_job_update(job).await;
        Ok(())
    }

    pub async fn get_job(&self, job_id: &str) -> Result<Option<Job>, SchedulerError> {
        let row = sqlx::query("SELECT * FROM jobs WHERE job_id = $1")
            .bind(job_id)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(r) = row {
            let status_str: String = r.get("status");
            let status: JobStatus =
                serde_json::from_str(&format!("\"{}\"", status_str)).unwrap_or_default();
            let env_vars: serde_json::Value = r.get("env_vars");
            let env: HashMap<String, String> = serde_json::from_value(env_vars).unwrap_or_default();

            let deployment_id: Option<String> = r.get("deployment_id");

            Ok(Some(Job {
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
                deployment_id,
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
                    volumes: vec![], // TODO: Persistent volumes support
                },
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn update_job_status(
        &self,
        job_id: &str,
        status: JobStatus,
    ) -> Result<(), SchedulerError> {
        let status_str = serde_json::to_string(&status)
            .unwrap_or_else(|_| "\"pending\"".to_string())
            .trim_matches('"')
            .to_string();

        sqlx::query("UPDATE jobs SET status = $1 WHERE job_id = $2")
            .bind(status_str)
            .bind(job_id)
            .execute(&self.pool)
            .await?;

        if let Some(job) = self.get_job(job_id).await? {
            self.notify_job_update(job).await;
        }
        Ok(())
    }

    pub async fn update_job_ip(&self, job_id: &str, ip: String) -> Result<(), SchedulerError> {
        sqlx::query("UPDATE jobs SET ip_address = $1 WHERE job_id = $2")
            .bind(ip)
            .bind(job_id)
            .execute(&self.pool)
            .await?;

        if let Some(job) = self.get_job(job_id).await? {
            self.notify_job_update(job).await;
        }
        Ok(())
    }

    pub async fn start_job(&self, job_id: &str) -> Result<(), SchedulerError> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query("UPDATE jobs SET status = 'running', started_at = $1 WHERE job_id = $2")
            .bind(now)
            .bind(job_id)
            .execute(&self.pool)
            .await?;

        if let Some(job) = self.get_job(job_id).await? {
            self.notify_job_update(job).await;
        }
        Ok(())
    }

    pub async fn fail_job(&self, job_id: &str, msg: String) -> Result<(), SchedulerError> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "UPDATE jobs SET status = 'failed', error_message = $1, stopped_at = $2 WHERE job_id = $3"
        )
        .bind(msg)
        .bind(now)
        .bind(job_id)
        .execute(&self.pool)
        .await?;

        if let Some(job) = self.get_job(job_id).await? {
            self.notify_job_update(job).await;
        }
        Ok(())
    }

    pub async fn cancel_job(&self, job_id: &str) -> Result<(), SchedulerError> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query("UPDATE jobs SET status = 'cancelled', stopped_at = $1 WHERE job_id = $2")
            .bind(now)
            .bind(job_id)
            .execute(&self.pool)
            .await?;

        if let Some(job) = self.get_job(job_id).await? {
            self.notify_job_update(job).await;
        }
        Ok(())
    }

    pub async fn remove_job(&self, job_id: &str) -> Result<bool, SchedulerError> {
        if let Some(job) = self.get_job(job_id).await? {
            if let Some(ref ip) = job.config.ip_address
                && let Some(ref host_id) = job.host_id
                && let Some(worker) = self.worker_registry.get_worker(host_id).await?
            {
                worker.ipam.release(ip).await?;
            }

            sqlx::query("DELETE FROM jobs WHERE job_id = $1")
                .bind(job_id)
                .execute(&self.pool)
                .await?;

            // Notify that the job is gone
            let mut final_job = job;
            final_job.status = JobStatus::Cancelled;
            self.notify_job_update(final_job).await;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn list_jobs(
        &self,
        user_id: Option<&str>,
        _status: Option<JobStatus>,
    ) -> Result<Vec<Job>, SchedulerError> {
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

        let mut jobs = Vec::new();
        for r in rows {
            let status_str: String = r.get("status");
            let status: JobStatus =
                serde_json::from_str(&format!("\"{}\"", status_str)).unwrap_or_default();
            let env_vars: serde_json::Value = r.get("env_vars");
            let env: HashMap<String, String> = serde_json::from_value(env_vars).unwrap_or_default();

            jobs.push(Job {
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
                    volumes: vec![],
                },
            });
        }
        Ok(jobs)
    }

    pub async fn select_best_worker(
        &self,
        config: &VmConfig,
        app_id: &str,
    ) -> Result<Worker, SchedulerError> {
        let workers = self.worker_registry.get_available_workers().await?;

        tracing::info!(
            available_workers = %workers.len(),
            "Selecting best worker from pool"
        );

        if workers.is_empty() {
            return Err(SchedulerError::NoWorkers);
        }

        let mut viable_workers: Vec<Worker> = workers
            .into_iter()
            .filter(|w| {
                if let Some(ref metrics) = w.metrics {
                    metrics.can_fit_vm(config.memory_mib, config.disk_mib)
                } else {
                    false
                }
            })
            .collect();

        if viable_workers.is_empty() {
            return Err(SchedulerError::NoFit);
        }

        // Count current app instances per worker for anti-affinity
        let jobs = self.list_jobs(None, None).await?;
        let mut app_counts_per_host: HashMap<String, u32> = HashMap::new();
        for job in jobs {
            if job.app_id == app_id
                && job.status != JobStatus::Failed
                && job.status != JobStatus::Cancelled
                && let Some(host_id) = &job.host_id
            {
                *app_counts_per_host.entry(host_id.clone()).or_insert(0) += 1;
            }
        }

        viable_workers.sort_by(|a, b| {
            let score_a = a
                .metrics
                .as_ref()
                .map_or(0.0, |m| m.calculate_score(MAX_APPS_PER_HOST));
            let score_b = b
                .metrics
                .as_ref()
                .map_or(0.0, |m| m.calculate_score(MAX_APPS_PER_HOST));

            // Apply soft anti-affinity penalty (each existing instance reduces score by 0.2)
            let penalty_a = (*app_counts_per_host.get(&a.host_id).unwrap_or(&0) as f32) * 0.2;
            let penalty_b = (*app_counts_per_host.get(&b.host_id).unwrap_or(&0) as f32) * 0.2;

            let final_a = (score_a - penalty_a).max(0.0);
            let final_b = (score_b - penalty_b).max(0.0);

            match self.strategy {
                SchedulingStrategy::LeastLoaded => final_b.partial_cmp(&final_a),
                SchedulingStrategy::BinPacking => final_a.partial_cmp(&final_b),
            }
            .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(viable_workers.remove(0))
    }

    pub async fn find_job_by_vm_id(&self, vm_id: &str) -> Result<Option<Job>, SchedulerError> {
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
