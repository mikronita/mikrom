use crate::scheduler::ipam::Ipam;
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
    pub bridge_ip: String,
    pub ipam: Ipam,
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
    #[must_use]
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
        bridge_ip: String,
    ) -> bool {
        let now = chrono::Utc::now().timestamp();

        let mut workers = self.workers.write();

        // Find if we already have this worker (by hostname or IP) to preserve IPAM state
        let existing_ipam = workers
            .values()
            .find(|w| w.hostname == hostname || w.ip_address == ip_address)
            .and_then(|w| {
                if w.bridge_ip == bridge_ip {
                    Some(w.ipam.clone())
                } else {
                    None
                }
            });

        let ipam = existing_ipam.unwrap_or_else(|| Ipam::new(&bridge_ip));

        let worker = Worker {
            host_id: host_id.clone(),
            hostname: hostname.clone(),
            ip_address: ip_address.clone(),
            agent_port,
            bridge_ip,
            ipam,
            channel: None,
            metrics: None,
            registered_at: now,
            last_heartbeat: now,
        };

        // Remove any stale worker with the same hostname but different host_id
        let stale_ids: Vec<String> = workers
            .values()
            .filter(|w| w.hostname == hostname && w.host_id != host_id)
            .map(|w| w.host_id.clone())
            .collect();

        for id in stale_ids {
            tracing::info!(
                "Removing stale worker registration for hostname {}: {}",
                hostname,
                id
            );
            workers.remove(&id);
        }

        workers.insert(host_id, worker);
        true
    }

    #[must_use]
    pub fn unregister(&self, host_id: &str) -> bool {
        let mut workers = self.workers.write();
        workers.remove(host_id).is_some()
    }

    #[must_use]
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

    #[must_use]
    pub fn get_worker(&self, host_id: &str) -> Option<Worker> {
        self.workers.read().get(host_id).cloned()
    }

    #[must_use]
    pub fn list_workers(&self) -> Vec<Worker> {
        self.workers.read().values().cloned().collect()
    }

    #[must_use]
    pub fn get_available_workers(&self) -> Vec<Worker> {
        let now = chrono::Utc::now().timestamp();
        self.workers
            .read()
            .values()
            .filter(|w| {
                w.metrics.is_some() && (now - w.last_heartbeat) < 30 // 30 seconds staleness threshold
            })
            .cloned()
            .collect()
    }

    #[must_use]
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
            load_avg_1: 0.0,
            load_avg_5: 0.0,
            load_avg_15: 0.0,
            vms: HashMap::new(),
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
            "10.0.1.1/24".to_string(),
        );
        assert!(ok);
        assert!(registry.is_registered("h1"));
        let w = registry.get_worker("h1").unwrap();
        assert_eq!(w.bridge_ip, "10.0.1.1/24");
    }

    #[test]
    fn test_register_overwrites_existing() {
        let registry = WorkerRegistry::new();
        registry.register(
            "h1".to_string(),
            "node1".to_string(),
            "10.0.0.1".to_string(),
            5003,
            "10.0.1.1/24".to_string(),
        );
        registry.register(
            "h1".to_string(),
            "node1-v2".to_string(),
            "10.0.0.9".to_string(),
            5003,
            "10.0.2.1/24".to_string(),
        );
        let w = registry.get_worker("h1").unwrap();
        assert_eq!(w.ip_address, "10.0.0.9");
        assert_eq!(w.bridge_ip, "10.0.2.1/24");
    }

    #[test]
    fn test_unregister_existing_worker() {
        let registry = WorkerRegistry::new();
        registry.register(
            "h1".to_string(),
            "node1".to_string(),
            "10.0.0.1".to_string(),
            5003,
            "10.0.1.1/24".to_string(),
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
            "172.16.0.1/16".to_string(),
        );
        let w = registry.get_worker("h1").unwrap();
        assert_eq!(w.host_id, "h1");
        assert_eq!(w.hostname, "mynode");
        assert_eq!(w.ip_address, "192.168.1.1");
        assert_eq!(w.agent_port, 6000);
        assert_eq!(w.bridge_ip, "172.16.0.1/16");
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
            "10.0.0.1/8".to_string(),
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
            "10.0.1.1/24".to_string(),
        );
        registry.register(
            "h2".to_string(),
            "n2".to_string(),
            "1.1.1.2".to_string(),
            5003,
            "10.0.2.1/24".to_string(),
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
            "10.0.1.1/24".to_string(),
        );
        registry.register(
            "h2".to_string(),
            "n2".to_string(),
            "1.1.1.2".to_string(),
            5003,
            "10.0.2.1/24".to_string(),
        );
        let _ = registry.update_metrics("h1", sample_metrics());

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
            "10.0.1.1/24".to_string(),
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
            "10.0.1.1/24".to_string(),
        );
        let _ = registry.unregister("h1");
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
            "10.0.1.1/24".to_string(),
        );
        let before = registry.get_worker("h1").unwrap().last_heartbeat;
        std::thread::sleep(std::time::Duration::from_millis(10));
        let _ = registry.update_metrics("h1", sample_metrics());
        let after = registry.get_worker("h1").unwrap().last_heartbeat;
        assert!(after >= before);
    }

    // ── Concurrency ──────────────────────────────────────────────────────────

    #[test]
    fn test_concurrent_register_from_multiple_threads() {
        use std::sync::Arc;
        let registry = Arc::new(WorkerRegistry::new());
        let mut handles = vec![];

        for i in 0..20u16 {
            let reg = registry.clone();
            handles.push(std::thread::spawn(move || {
                reg.register(
                    format!("host-{i}"),
                    format!("node-{i}"),
                    "127.0.0.1".to_string(),
                    5000 + i,
                    format!("10.0.{i}.1/24"),
                );
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(registry.list_workers().len(), 20);
        for i in 0..20 {
            assert!(registry.is_registered(&format!("host-{i}")));
        }
    }

    #[test]
    fn test_concurrent_update_metrics_from_multiple_threads() {
        use std::sync::Arc;
        let registry = Arc::new(WorkerRegistry::new());
        // Pre-register 10 workers.
        for i in 0..10u16 {
            registry.register(
                format!("h{i}"),
                format!("n{i}"),
                "127.0.0.1".to_string(),
                5000 + i,
                format!("10.0.{i}.1/24"),
            );
        }

        let mut handles = vec![];
        for i in 0..10u16 {
            let reg = registry.clone();
            handles.push(std::thread::spawn(move || {
                let ok = reg.update_metrics(
                    &format!("h{i}"),
                    HostMetrics {
                        cpu_usage: f32::from(i) * 0.1,
                        ram_used_bytes: (u64::from(i) + 1) * 100_000_000,
                        ram_total_bytes: 8 * 1024 * 1024 * 1024,
                        disk_used_bytes: 0,
                        disk_total_bytes: 100 * 1024 * 1024 * 1024,
                        apps_count: u32::from(i),
                        load_avg_1: 0.0,
                        load_avg_5: 0.0,
                        load_avg_15: 0.0,
                        vms: HashMap::new(),
                        timestamp: 0,
                    },
                );
                assert!(ok);
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        let available = registry.get_available_workers();
        assert_eq!(available.len(), 10);
    }

    #[test]
    fn test_concurrent_register_and_unregister() {
        use std::sync::Arc;
        let registry = Arc::new(WorkerRegistry::new());
        // Register 10 workers first.
        for i in 0..10u16 {
            registry.register(
                format!("h{i}"),
                format!("n{i}"),
                "127.0.0.1".to_string(),
                5000 + i,
                format!("10.0.{i}.1/24"),
            );
        }

        let mut handles = vec![];
        // 5 threads register new workers.
        for i in 10..15u16 {
            let reg = registry.clone();
            handles.push(std::thread::spawn(move || {
                reg.register(
                    format!("h{i}"),
                    format!("n{i}"),
                    "127.0.0.1".to_string(),
                    5000 + i,
                    format!("10.0.{i}.1/24"),
                );
            }));
        }
        // 5 threads unregister the pre-registered workers.
        for i in 0..5u16 {
            let reg = registry.clone();
            handles.push(std::thread::spawn(move || {
                let _ = reg.unregister(&format!("h{i}"));
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        // Workers h0..h4 were unregistered; h5..h14 should remain.
        let count = registry.list_workers().len();
        assert_eq!(count, 10); // 5 old survivors + 5 new
    }

    #[test]
    fn test_concurrent_read_while_writing() {
        use std::sync::Arc;
        let registry = Arc::new(WorkerRegistry::new());
        for i in 0..5u16 {
            registry.register(
                format!("h{i}"),
                format!("n{i}"),
                "127.0.0.1".to_string(),
                5000 + i,
                format!("10.0.{i}.1/24"),
            );
        }

        let mut handles = vec![];
        // Writers: update metrics concurrently.
        for i in 0..5u16 {
            let reg = registry.clone();
            handles.push(std::thread::spawn(move || {
                let _ = reg.update_metrics(&format!("h{i}"), sample_metrics());
            }));
        }
        // Readers: list workers concurrently.
        for _ in 0..5 {
            let reg = registry.clone();
            handles.push(std::thread::spawn(move || {
                let _ = reg.list_workers();
                let _ = reg.get_available_workers();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
    }
}
