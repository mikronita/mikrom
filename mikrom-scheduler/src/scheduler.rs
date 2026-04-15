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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::VmConfig;
    use crate::metrics::HostMetrics;
    use crate::worker_registry::WorkerRegistry;

    const GIB: u64 = 1024 * 1024 * 1024;

    fn make_scheduler() -> AppScheduler {
        AppScheduler::new(WorkerRegistry::new())
    }

    fn small_config() -> VmConfig {
        VmConfig { vcpus: 1, memory_mib: 256, disk_mib: 1024, env: Default::default() }
    }

    fn make_job(id: &str) -> Job {
        Job::new(id.to_string(), "app-1".to_string(), "my-app".to_string(),
            "nginx:latest".to_string(), small_config(), "user-1".to_string())
    }

    fn register_with_metrics(scheduler: &AppScheduler, host_id: &str, cpu: f32) {
        scheduler.worker_registry().register(
            host_id.to_string(), "node".to_string(), "10.0.0.1".to_string(), 5003,
        );
        scheduler.worker_registry().update_metrics(host_id, HostMetrics {
            cpu_usage: cpu,
            ram_used_bytes: 512 * 1024 * 1024,
            ram_total_bytes: 4 * GIB,
            disk_used_bytes: 10 * GIB,
            disk_total_bytes: 100 * GIB,
            apps_count: 1,
            timestamp: 0,
        });
    }

    #[test]
    fn test_add_and_get_job() {
        let s = make_scheduler();
        s.add_job(make_job("j1"));
        let found = s.get_job("j1");
        assert!(found.is_some());
        assert_eq!(found.unwrap().job_id, "j1");
    }

    #[test]
    fn test_get_job_missing_returns_none() {
        assert!(make_scheduler().get_job("ghost").is_none());
    }

    #[test]
    fn test_update_job_status() {
        let s = make_scheduler();
        s.add_job(make_job("j1"));
        s.update_job_status("j1", JobStatus::Running);
        assert_eq!(s.get_job("j1").unwrap().status, JobStatus::Running);
    }

    #[test]
    fn test_update_job_status_nonexistent_is_noop() {
        let s = make_scheduler();
        s.update_job_status("ghost", JobStatus::Failed); // must not panic
    }

    #[test]
    fn test_list_jobs_returns_all_when_no_filter() {
        let s = make_scheduler();
        s.add_job(make_job("j1"));
        s.add_job(make_job("j2"));
        assert_eq!(s.list_jobs(None, None).len(), 2);
    }

    #[test]
    fn test_list_jobs_filtered_by_user() {
        let s = make_scheduler();
        let mut j1 = make_job("j1");
        j1.user_id = "alice".to_string();
        let mut j2 = make_job("j2");
        j2.user_id = "bob".to_string();
        s.add_job(j1);
        s.add_job(j2);

        let alice_jobs = s.list_jobs(Some("alice"), None);
        assert_eq!(alice_jobs.len(), 1);
        assert_eq!(alice_jobs[0].user_id, "alice");
    }

    #[test]
    fn test_list_jobs_user_with_no_jobs_returns_empty() {
        let s = make_scheduler();
        s.add_job(make_job("j1"));
        assert_eq!(s.list_jobs(Some("nobody"), None).len(), 0);
    }

    #[test]
    fn test_select_best_worker_no_workers() {
        let s = make_scheduler();
        let result = s.select_best_worker(&small_config());
        assert!(matches!(result, Err(SchedulerError::NoWorkers)));
    }

    #[test]
    fn test_select_best_worker_worker_without_metrics_is_unavailable() {
        let s = make_scheduler();
        s.worker_registry().register("h1".to_string(), "n".to_string(), "1.1.1.1".to_string(), 5003);
        // no metrics → get_available_workers returns empty → NoWorkers
        let result = s.select_best_worker(&small_config());
        assert!(matches!(result, Err(SchedulerError::NoWorkers)));
    }

    #[test]
    fn test_select_best_worker_no_fit() {
        let s = make_scheduler();
        register_with_metrics(&s, "h1", 0.1);
        // Request more RAM than the worker has available
        let huge = VmConfig { vcpus: 1, memory_mib: 100_000, disk_mib: 1024, env: Default::default() };
        assert!(matches!(s.select_best_worker(&huge), Err(SchedulerError::NoFit)));
    }

    #[test]
    fn test_select_best_worker_success() {
        let s = make_scheduler();
        register_with_metrics(&s, "h1", 0.1);
        let result = s.select_best_worker(&small_config());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().host_id, "h1");
    }

    #[test]
    fn test_select_best_worker_picks_highest_score() {
        let s = make_scheduler();
        register_with_metrics(&s, "busy-host", 0.9);   // low score
        register_with_metrics(&s, "idle-host", 0.05);  // high score
        let winner = s.select_best_worker(&small_config()).unwrap();
        assert_eq!(winner.host_id, "idle-host");
    }

    #[test]
    fn test_find_job_by_vm_id() {
        let s = make_scheduler();
        let mut job = make_job("j1");
        job.schedule("h1".to_string(), "vm-abc".to_string());
        s.add_job(job);
        let found = s.find_job_by_vm_id("vm-abc");
        assert!(found.is_some());
        assert_eq!(found.unwrap().job_id, "j1");
    }

    #[test]
    fn test_find_job_by_vm_id_not_found() {
        assert!(make_scheduler().find_job_by_vm_id("ghost-vm").is_none());
    }

    #[test]
    fn test_scheduler_error_display() {
        assert_eq!(SchedulerError::NoWorkers.to_string(), "No available workers");
        assert_eq!(SchedulerError::NoFit.to_string(), "No worker can fit the VM requirements");
        assert!(SchedulerError::JobNotFound("j1".to_string()).to_string().contains("j1"));
    }
}
