use crate::job::{Job, JobStatus, VmConfig};
use crate::worker_registry::{Worker, WorkerRegistry};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

pub mod ipam;

#[derive(Error, Debug)]
pub enum SchedulerError {
    #[error("No available workers")]
    NoWorkers,
    #[error("No worker can fit the VM requirements")]
    NoFit,
    #[error("Job not found: {0}")]
    JobNotFound(String),
    #[error("IP address pool exhausted")]
    IpPoolExhausted,
}

const MAX_APPS_PER_HOST: u32 = 10;

#[derive(Debug, Clone, Copy, Default)]
pub enum SchedulingStrategy {
    #[default]
    LeastLoaded, // Current behavior: spreads load
    BinPacking, // Fill nodes sequentially to optimize costs
}

#[derive(Clone)]
pub struct AppScheduler {
    pub worker_registry: WorkerRegistry,
    pub jobs: Arc<RwLock<HashMap<String, Job>>>,
    pub strategy: SchedulingStrategy,
}

impl AppScheduler {
    pub fn new(worker_registry: WorkerRegistry) -> Self {
        Self {
            worker_registry,
            jobs: Arc::new(RwLock::new(HashMap::new())),
            strategy: SchedulingStrategy::default(),
        }
    }

    pub fn with_strategy(mut self, strategy: SchedulingStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    pub fn worker_registry(&self) -> &WorkerRegistry {
        &self.worker_registry
    }

    pub fn add_job(&self, job: Job) {
        let job_id = job.job_id.clone();
        let job_clone = job.clone();
        self.jobs.write().insert(job_id, job_clone);
    }

    pub fn get_job(&self, job_id: &str) -> Option<Job> {
        self.jobs.read().get(job_id).cloned()
    }

    pub fn update_job_status(&self, job_id: &str, status: JobStatus) {
        if let Some(job) = self.jobs.write().get_mut(job_id) {
            job.status = status;
        }
    }

    pub fn start_job(&self, job_id: &str) {
        if let Some(job) = self.jobs.write().get_mut(job_id) {
            job.start();
        }
    }

    pub fn fail_job(&self, job_id: &str, msg: String) {
        if let Some(job) = self.jobs.write().get_mut(job_id) {
            job.fail(msg);
        }
    }

    pub fn cancel_job(&self, job_id: &str) {
        if let Some(job) = self.jobs.write().get_mut(job_id) {
            job.cancel();
        }
    }

    pub fn remove_job(&self, job_id: &str) -> bool {
        if let Some(job) = self.jobs.write().remove(job_id) {
            if let Some(ip) = job.config.ip_address {
                if let Some(host_id) = job.host_id {
                    if let Some(worker) = self.worker_registry.get_worker(&host_id) {
                        worker.ipam.release(&ip);
                    }
                }
            }
            true
        } else {
            false
        }
    }

    pub fn list_jobs(&self, user_id: Option<&str>, _status: Option<JobStatus>) -> Vec<Job> {
        let jobs = self.jobs.read();
        jobs.values()
            .filter(|j| user_id.map(|u| j.user_id == u).unwrap_or(true))
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
                .map(|m| m.calculate_score(MAX_APPS_PER_HOST))
                .unwrap_or(0.0);
            let score_b = b
                .metrics
                .as_ref()
                .map(|m| m.calculate_score(MAX_APPS_PER_HOST))
                .unwrap_or(0.0);

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
            format!("host-{}", id),
            "127.0.0.1".to_string(),
            5003,
            "10.0.0.1/8".to_string(),
        );

        let mut metrics = HostMetrics::default();
        metrics.cpu_usage = cpu;
        metrics.ram_total_bytes = 4096 * 1024 * 1024;
        metrics.ram_used_bytes = metrics.ram_total_bytes - (ram_free_mb * 1024 * 1024);
        metrics.apps_count = 0;

        registry.update_metrics(id, metrics);
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
