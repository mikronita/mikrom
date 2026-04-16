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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::HostMetrics;

    fn sample_metrics() -> HostMetrics {
        HostMetrics {
            cpu_usage: 0.1,
            ram_used_bytes: 512 * 1024 * 1024,
            ram_total_bytes: 4 * 1024 * 1024 * 1024,
            disk_used_bytes: 10 * 1024 * 1024 * 1024,
            disk_total_bytes: 100 * 1024 * 1024 * 1024,
            apps_count: 1,
            timestamp: 0,
        }
    }

    #[test]
    fn test_register_new_worker() {
        let registry = WorkerRegistry::new();
        let ok = registry.register(
            "h1".to_string(),
            "node1".to_string(),
            "10.0.0.1".to_string(),
            5003,
        );
        assert!(ok);
        assert!(registry.is_registered("h1"));
    }

    #[test]
    fn test_register_overwrites_existing() {
        let registry = WorkerRegistry::new();
        registry.register(
            "h1".to_string(),
            "node1".to_string(),
            "10.0.0.1".to_string(),
            5003,
        );
        registry.register(
            "h1".to_string(),
            "node1-v2".to_string(),
            "10.0.0.9".to_string(),
            5003,
        );
        let w = registry.get_worker("h1").unwrap();
        assert_eq!(w.ip_address, "10.0.0.9");
    }

    #[test]
    fn test_unregister_existing_worker() {
        let registry = WorkerRegistry::new();
        registry.register(
            "h1".to_string(),
            "node1".to_string(),
            "10.0.0.1".to_string(),
            5003,
        );
        assert!(registry.unregister("h1"));
        assert!(!registry.is_registered("h1"));
    }

    #[test]
    fn test_unregister_nonexistent_returns_false() {
        let registry = WorkerRegistry::new();
        assert!(!registry.unregister("ghost"));
    }

    #[test]
    fn test_get_worker_returns_correct_fields() {
        let registry = WorkerRegistry::new();
        registry.register(
            "h1".to_string(),
            "mynode".to_string(),
            "192.168.1.1".to_string(),
            6000,
        );
        let w = registry.get_worker("h1").unwrap();
        assert_eq!(w.host_id, "h1");
        assert_eq!(w.hostname, "mynode");
        assert_eq!(w.ip_address, "192.168.1.1");
        assert_eq!(w.agent_port, 6000);
        assert!(w.metrics.is_none());
    }

    #[test]
    fn test_get_worker_missing_returns_none() {
        let registry = WorkerRegistry::new();
        assert!(registry.get_worker("ghost").is_none());
    }

    #[test]
    fn test_update_metrics_success() {
        let registry = WorkerRegistry::new();
        registry.register(
            "h1".to_string(),
            "n".to_string(),
            "1.2.3.4".to_string(),
            5003,
        );
        assert!(registry.update_metrics("h1", sample_metrics()));
        let w = registry.get_worker("h1").unwrap();
        assert!(w.metrics.is_some());
        assert!((w.metrics.unwrap().cpu_usage - 0.1).abs() < 0.001);
    }

    #[test]
    fn test_update_metrics_nonexistent_returns_false() {
        let registry = WorkerRegistry::new();
        assert!(!registry.update_metrics("ghost", sample_metrics()));
    }

    #[test]
    fn test_list_workers() {
        let registry = WorkerRegistry::new();
        registry.register(
            "h1".to_string(),
            "n1".to_string(),
            "1.1.1.1".to_string(),
            5003,
        );
        registry.register(
            "h2".to_string(),
            "n2".to_string(),
            "1.1.1.2".to_string(),
            5003,
        );
        assert_eq!(registry.list_workers().len(), 2);
    }

    #[test]
    fn test_list_workers_empty() {
        let registry = WorkerRegistry::new();
        assert!(registry.list_workers().is_empty());
    }

    #[test]
    fn test_get_available_workers_only_includes_those_with_metrics() {
        let registry = WorkerRegistry::new();
        registry.register(
            "h1".to_string(),
            "n1".to_string(),
            "1.1.1.1".to_string(),
            5003,
        );
        registry.register(
            "h2".to_string(),
            "n2".to_string(),
            "1.1.1.2".to_string(),
            5003,
        );
        registry.update_metrics("h1", sample_metrics());

        let available = registry.get_available_workers();
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].host_id, "h1");
    }

    #[test]
    fn test_get_available_workers_empty_when_no_metrics() {
        let registry = WorkerRegistry::new();
        registry.register(
            "h1".to_string(),
            "n1".to_string(),
            "1.1.1.1".to_string(),
            5003,
        );
        assert!(registry.get_available_workers().is_empty());
    }

    #[test]
    fn test_is_registered_false_after_unregister() {
        let registry = WorkerRegistry::new();
        registry.register(
            "h1".to_string(),
            "n".to_string(),
            "1.1.1.1".to_string(),
            5003,
        );
        registry.unregister("h1");
        assert!(!registry.is_registered("h1"));
    }

    #[test]
    fn test_update_metrics_updates_last_heartbeat() {
        let registry = WorkerRegistry::new();
        registry.register(
            "h1".to_string(),
            "n".to_string(),
            "1.1.1.1".to_string(),
            5003,
        );
        let before = registry.get_worker("h1").unwrap().last_heartbeat;
        std::thread::sleep(std::time::Duration::from_millis(10));
        registry.update_metrics("h1", sample_metrics());
        let after = registry.get_worker("h1").unwrap().last_heartbeat;
        assert!(after >= before);
    }
}
