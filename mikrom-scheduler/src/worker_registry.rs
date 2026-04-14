use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

use tonic::transport::Channel;

#[derive(Clone, Debug)]
pub struct Worker {
    pub host_id: String,
    pub hostname: String,
    pub ip_address: String,
    pub agent_port: u16,
    pub channel: Option<Channel>,
    pub metrics: Option<HostMetrics>,
    pub registered_at: i64,
    pub last_heartbeat: i64,
}

pub use crate::metrics::HostMetrics;

#[derive(Clone)]
pub struct WorkerRegistry {
    workers: Arc<RwLock<HashMap<String, Worker>>>,
}

impl WorkerRegistry {
    pub fn new() -> Self {
        Self {
            workers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn register(
        &self,
        host_id: String,
        hostname: String,
        ip_address: String,
        agent_port: u16,
    ) -> bool {
        let now = chrono::Utc::now().timestamp();
        let worker = Worker {
            host_id: host_id.clone(),
            hostname,
            ip_address,
            agent_port,
            channel: None,
            metrics: None,
            registered_at: now,
            last_heartbeat: now,
        };

        let mut workers = self.workers.write();
        workers.insert(host_id, worker);
        true
    }

    pub fn unregister(&self, host_id: &str) -> bool {
        let mut workers = self.workers.write();
        workers.remove(host_id).is_some()
    }

    pub fn update_metrics(&self, host_id: &str, metrics: HostMetrics) -> bool {
        let mut workers = self.workers.write();
        if let Some(worker) = workers.get_mut(host_id) {
            worker.metrics = Some(metrics);
            worker.last_heartbeat = chrono::Utc::now().timestamp();
            true
        } else {
            false
        }
    }

    pub fn get_worker(&self, host_id: &str) -> Option<Worker> {
        self.workers.read().get(host_id).cloned()
    }

    pub fn list_workers(&self) -> Vec<Worker> {
        self.workers.read().values().cloned().collect()
    }

    pub fn get_available_workers(&self) -> Vec<Worker> {
        self.workers
            .read()
            .values()
            .filter(|w| w.metrics.is_some())
            .cloned()
            .collect()
    }

    pub fn is_registered(&self, host_id: &str) -> bool {
        self.workers.read().contains_key(host_id)
    }
}

impl Default for WorkerRegistry {
    fn default() -> Self {
        Self::new()
    }
}
