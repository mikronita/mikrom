use crate::job::{Job, JobStatus, VmConfig};
use crate::worker_registry::{Worker, WorkerRegistry};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SchedulerError {
    #[error("No available workers")]
    NoWorkers,
    #[error("No worker can fit the VM requirements")]
    NoFit,
    #[error("Job not found: {0}")]
    JobNotFound(String),
}

const MAX_APPS_PER_HOST: u32 = 10;

#[derive(Clone)]
pub struct AppScheduler {
    worker_registry: WorkerRegistry,
    jobs: Arc<RwLock<HashMap<String, Job>>>,
}

impl AppScheduler {
    pub fn new(worker_registry: WorkerRegistry) -> Self {
        Self {
            worker_registry,
            jobs: Arc::new(RwLock::new(HashMap::new())),
        }
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

    pub fn list_jobs(&self, user_id: Option<&str>, _status: Option<JobStatus>) -> Vec<Job> {
        let jobs = self.jobs.read();
        jobs.values()
            .filter(|j| {
                let user_match = user_id.map(|u| j.user_id == u).unwrap_or(true);
                user_match
            })
            .cloned()
            .collect()
    }

    pub fn select_best_worker(&self, config: &VmConfig) -> Result<Worker, SchedulerError> {
        let workers = self.worker_registry.get_available_workers();

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

            score_b
                .partial_cmp(&score_a)
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
