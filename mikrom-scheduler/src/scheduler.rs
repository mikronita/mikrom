use crate::job::{Job, JobStatus, VmConfig};
use crate::worker_registry::{Worker, WorkerRegistry};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
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
}

const MAX_APPS_PER_HOST: u32 = 10;

/// Strategies for selecting a worker to host a new VM.
#[derive(Debug, Clone, Copy, Default)]
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
    /// Registry of all active workers and their resource availability.
    pub worker_registry: WorkerRegistry,
    /// In-memory store of all jobs managed by this scheduler.
    pub jobs: Arc<RwLock<HashMap<String, Job>>>,
    /// The strategy used for placing new workloads.
    pub strategy: SchedulingStrategy,
    /// Broadcast channel for job updates.
    pub job_updates: tokio::sync::broadcast::Sender<Job>,
}

impl AppScheduler {
    /// Creates a new scheduler with the provided worker registry.
    #[must_use]
    pub fn new(worker_registry: WorkerRegistry) -> Self {
        let (job_updates, _) = tokio::sync::broadcast::channel(1024);
        Self {
            worker_registry,
            jobs: Arc::new(RwLock::new(HashMap::new())),
            strategy: SchedulingStrategy::default(),
            job_updates,
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

    pub fn add_job(&self, job: Job) {
        let job_id = job.job_id.clone();
        let job_clone = job.clone();
        self.jobs.write().insert(job_id, job_clone);
        let _ = self.job_updates.send(job);
    }

    #[must_use]
    pub fn get_job(&self, job_id: &str) -> Option<Job> {
        self.jobs.read().get(job_id).cloned()
    }

    pub fn update_job_status(&self, job_id: &str, status: JobStatus) {
        if let Some(job) = self.jobs.write().get_mut(job_id) {
            job.status = status;
            let _ = self.job_updates.send(job.clone());
        }
    }

    pub fn update_job_ip(&self, job_id: &str, ip: String) {
        if let Some(job) = self.jobs.write().get_mut(job_id) {
            job.config.ip_address = Some(ip);
            let _ = self.job_updates.send(job.clone());
        }
    }

    pub fn start_job(&self, job_id: &str) {
        if let Some(job) = self.jobs.write().get_mut(job_id) {
            job.start();
            let _ = self.job_updates.send(job.clone());
        }
    }

    pub fn fail_job(&self, job_id: &str, msg: String) {
        if let Some(job) = self.jobs.write().get_mut(job_id) {
            job.fail(msg);
            let _ = self.job_updates.send(job.clone());
        }
    }

    pub fn cancel_job(&self, job_id: &str) {
        if let Some(job) = self.jobs.write().get_mut(job_id) {
            job.cancel();
            let _ = self.job_updates.send(job.clone());
        }
    }

    #[must_use]
    pub fn remove_job(&self, job_id: &str) -> bool {
        if let Some(job) = self.jobs.write().remove(job_id) {
            if let Some(ref ip) = job.config.ip_address
                && let Some(ref host_id) = job.host_id
                && let Some(worker) = self.worker_registry.get_worker(host_id)
            {
                worker.ipam.release(ip);
            }
            // Notify that the job is gone
            let mut final_job = job;
            final_job.status = JobStatus::Cancelled;
            let _ = self.job_updates.send(final_job);
            true
        } else {
            false
        }
    }

    #[must_use]
    pub fn list_jobs(&self, user_id: Option<&str>, _status: Option<JobStatus>) -> Vec<Job> {
        let jobs = self.jobs.read();
        jobs.values()
            .filter(|j| user_id.is_none_or(|u| j.user_id == u))
            .cloned()
            .collect()
    }

    pub fn select_best_worker(
        &self,
        config: &VmConfig,
        app_id: &str,
    ) -> Result<Worker, SchedulerError> {
        let workers = self.worker_registry.get_available_workers();

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
        let jobs = self.jobs.read();
        let mut app_counts_per_host: HashMap<String, u32> = HashMap::new();
        for job in jobs.values() {
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

    #[must_use]
    pub fn find_job_by_vm_id(&self, vm_id: &str) -> Option<Job> {
        self.jobs
            .read()
            .values()
            .find(|j| j.vm_id.as_deref() == Some(vm_id))
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::HostMetrics;

    fn register_worker_with_metrics(
        registry: &WorkerRegistry,
        id: &str,
        cpu: f32,
        ram_free_mb: u64,
    ) {
        registry.register(
            id.to_string(),
            format!("host-{id}"),
            "127.0.0.1".to_string(),
            5003,
            "10.0.0.1/8".to_string(),
        );

        let mut metrics = HostMetrics::default();
        metrics.cpu_usage = cpu;
        metrics.ram_total_bytes = 4096 * 1024 * 1024;
        metrics.ram_used_bytes = metrics.ram_total_bytes - (ram_free_mb * 1024 * 1024);
        metrics.apps_count = 0;

        let _ = registry.update_metrics(id, metrics);
    }

    #[test]
    fn test_strategy_least_loaded() {
        let registry = WorkerRegistry::new();
        // Host A: 10% CPU, 2000MB free
        register_worker_with_metrics(&registry, "A", 0.1, 2000);
        // Host B: 50% CPU, 500MB free
        register_worker_with_metrics(&registry, "B", 0.5, 500);

        let scheduler = AppScheduler::new(registry).with_strategy(SchedulingStrategy::LeastLoaded);
        let config = VmConfig {
            memory_mib: 256,
            disk_mib: 1024,
            ..Default::default()
        };

        let best = scheduler.select_best_worker(&config, "app-1").unwrap();
        assert_eq!(best.host_id, "A");
    }

    #[test]
    fn test_strategy_bin_packing() {
        let registry = WorkerRegistry::new();
        // Host A: 10% CPU, 2000MB free (Least loaded)
        register_worker_with_metrics(&registry, "A", 0.1, 2000);
        // Host B: 50% CPU, 500MB free (More loaded, but fits)
        register_worker_with_metrics(&registry, "B", 0.5, 500);

        let scheduler = AppScheduler::new(registry).with_strategy(SchedulingStrategy::BinPacking);
        let config = VmConfig {
            memory_mib: 128,
            disk_mib: 1024,
            ..Default::default()
        };

        // Should pick B because it's more loaded but still fits (packing)
        let best = scheduler.select_best_worker(&config, "app-1").unwrap();
        assert_eq!(best.host_id, "B");
    }

    #[test]
    fn test_soft_anti_affinity() {
        let registry = WorkerRegistry::new();
        // Two identical hosts
        register_worker_with_metrics(&registry, "A", 0.1, 2000);
        register_worker_with_metrics(&registry, "B", 0.1, 2000);

        let scheduler = AppScheduler::new(registry);

        // Add a job for "app-1" already running on host A
        let mut job = Job::new(
            "j1".into(),
            "app-1".into(),
            "n".into(),
            "i".into(),
            VmConfig::default(),
            "u".into(),
        );
        job.host_id = Some("A".into());
        job.vm_id = Some("vm1".into());
        job.status = JobStatus::Running;
        scheduler.add_job(job);

        let config = VmConfig {
            memory_mib: 256,
            disk_mib: 1024,
            ..Default::default()
        };

        // Should pick B even if A is equally loaded, because app-1 is already on A
        let best = scheduler.select_best_worker(&config, "app-1").unwrap();
        assert_eq!(best.host_id, "B");
    }
}
