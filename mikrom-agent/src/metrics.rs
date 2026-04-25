use crate::firecracker::FirecrackerManager;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use sysinfo::System;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VmMetrics {
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
    pub status: crate::firecracker::VmStatus,
    pub error_message: Option<String>,
    pub ip_address: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SystemMetrics {
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
    pub ram_total_bytes: u64,
    pub disk_used_bytes: u64,
    pub disk_total_bytes: u64,
    pub apps_count: u32,
    pub load_avg_1: f32,
    pub load_avg_5: f32,
    pub load_avg_15: f32,
    pub vms: HashMap<String, VmMetrics>,
    pub timestamp: i64,
}

impl Default for SystemMetrics {
    fn default() -> Self {
        Self {
            cpu_usage: 0.0,
            ram_used_bytes: 0,
            ram_total_bytes: 0,
            disk_used_bytes: 0,
            disk_total_bytes: 0,
            apps_count: 0,
            load_avg_1: 0.0,
            load_avg_5: 0.0,
            load_avg_15: 0.0,
            vms: HashMap::new(),
            timestamp: chrono::Utc::now().timestamp(),
        }
    }
}

#[derive(Clone)]
pub struct MetricsCollector {
    system: Arc<RwLock<System>>,
    apps_count: Arc<RwLock<u32>>,
    cached_metrics: Arc<RwLock<Option<(SystemMetrics, i64)>>>,
    firecracker: Option<FirecrackerManager>,
}

impl MetricsCollector {
    #[must_use]
    pub fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_all();

        Self {
            system: Arc::new(RwLock::new(system)),
            apps_count: Arc::new(RwLock::new(0)),
            cached_metrics: Arc::new(RwLock::new(None)),
            firecracker: None,
        }
    }

    #[must_use]
    pub fn with_firecracker(firecracker: FirecrackerManager) -> Self {
        let mut collector = Self::new();
        collector.firecracker = Some(firecracker);
        collector
    }

    pub async fn collect(&self) -> SystemMetrics {
        let now = chrono::Utc::now().timestamp();

        // 1 second cache
        if let Some((cached, timestamp)) = self.cached_metrics.read().as_ref()
            && (now - *timestamp) < 1
        {
            let mut metrics = cached.clone();
            // apps_count might have changed, update it from its own lock
            metrics.apps_count = *self.apps_count.read();
            metrics.timestamp = now;
            return metrics;
        }

        let vms_info = if let Some(mgr) = &self.firecracker {
            mgr.get_all_vms().await
        } else {
            Vec::new()
        };

        // ── Pre-collect: Flush Firecracker metrics ──────────────────────────
        let flush_body = serde_json::json!({
            "action_type": "FlushMetrics"
        })
        .to_string();

        let flush_futures: Vec<_> = vms_info
            .iter()
            .filter_map(|vm| {
                vm.socket_path
                    .as_ref()
                    .map(|socket| crate::firecracker::api::fc_put(socket, "/actions", &flush_body))
            })
            .collect();
        futures::future::join_all(flush_futures).await;

        let mut system_metrics = {
            let mut system = self.system.write();
            system.refresh_all();

            let cpu_usage = system.global_cpu_usage() / 100.0;
            let ram_used_bytes = system.used_memory();
            let ram_total_bytes = system.total_memory();

            let (disk_used_bytes, disk_total_bytes) = self.get_disk_usage();
            let apps_count = *self.apps_count.read();
            let load_avg = System::load_average();

            SystemMetrics {
                cpu_usage,
                ram_used_bytes,
                ram_total_bytes,
                disk_used_bytes,
                disk_total_bytes,
                apps_count,
                load_avg_1: load_avg.one as f32,
                load_avg_5: load_avg.five as f32,
                load_avg_15: load_avg.fifteen as f32,
                vms: HashMap::new(),
                timestamp: now,
            }
        };

        // Collect per-VM metrics
        let mut vms = HashMap::new();
        for vm in vms_info {
            let mut cpu = 0.0;
            let mut ram = 0;

            // Primary: Use sysinfo to get host-side process metrics (most reliable for cgroup-limited VMs)
            if let Some(pid) = vm.pid
                && pid > 0
            {
                let system = self.system.read();
                if let Some(process) = system.process(sysinfo::Pid::from(pid as usize)) {
                    cpu = process.cpu_usage() / 100.0;
                    ram = process.memory();
                }
            }

            // Secondary: Try to read Firecracker internal metrics if available
            if let Some(metrics_path) = &vm.metrics_path
                && let Ok(content) = tokio::fs::read_to_string(metrics_path).await
                && let Ok(_json) = serde_json::from_str::<serde_json::Value>(&content)
            {
                tracing::debug!(vm_id = %vm.vm_id, "Read Firecracker metrics successfully");
            }

            vms.insert(
                vm.vm_id,
                VmMetrics {
                    cpu_usage: cpu,
                    ram_used_bytes: ram,
                    status: vm.status,
                    error_message: vm.error_message,
                    ip_address: vm.ip_address,
                },
            );
        }

        system_metrics.vms = vms;
        let metrics = system_metrics;
        *self.cached_metrics.write() = Some((metrics.clone(), now));
        metrics
    }

    fn get_disk_usage(&self) -> (u64, u64) {
        let sys = sysinfo::Disks::new_with_refreshed_list();

        let mut total_space: u64 = 0;
        let mut available_space: u64 = 0;

        for disk in sys.list() {
            total_space += disk.total_space();
            available_space += disk.available_space();
        }

        let disk_used_bytes = total_space.saturating_sub(available_space);

        (disk_used_bytes, total_space)
    }

    pub fn increment_app_count(&self) {
        let mut count = self.apps_count.write();
        *count += 1;
    }

    pub fn decrement_app_count(&self) {
        let mut count = self.apps_count.write();
        *count = count.saturating_sub(1);
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::get_unwrap)]
mod tests {
    use super::*;

    #[test]
    fn test_system_metrics_default_zeroed() {
        let m = SystemMetrics::default();
        assert_eq!(m.cpu_usage, 0.0);
        assert_eq!(m.ram_used_bytes, 0);
        assert_eq!(m.ram_total_bytes, 0);
        assert_eq!(m.disk_used_bytes, 0);
        assert_eq!(m.disk_total_bytes, 0);
        assert_eq!(m.apps_count, 0);
        assert!(m.timestamp > 0);
    }

    #[tokio::test]
    async fn test_collect_returns_real_system_data() {
        let collector = MetricsCollector::new();
        let metrics = collector.collect().await;
        assert!(metrics.ram_total_bytes > 0, "total RAM must be > 0");
        assert!(metrics.timestamp > 0);
    }

    #[tokio::test]
    async fn test_cpu_usage_within_valid_range() {
        let collector = MetricsCollector::new();
        let metrics = collector.collect().await;
        assert!(metrics.cpu_usage >= 0.0);
        assert!(metrics.cpu_usage <= 1.0);
    }

    #[tokio::test]
    async fn test_ram_used_does_not_exceed_total() {
        let collector = MetricsCollector::new();
        let metrics = collector.collect().await;
        assert!(metrics.ram_used_bytes <= metrics.ram_total_bytes);
    }

    #[tokio::test]
    async fn test_increment_app_count() {
        let collector = MetricsCollector::new();
        collector.increment_app_count();
        collector.increment_app_count();
        assert_eq!(collector.collect().await.apps_count, 2);
    }

    #[tokio::test]
    async fn test_decrement_app_count() {
        let collector = MetricsCollector::new();
        collector.increment_app_count();
        collector.increment_app_count();
        collector.decrement_app_count();
        assert_eq!(collector.collect().await.apps_count, 1);
    }

    #[tokio::test]
    async fn test_decrement_saturates_at_zero() {
        let collector = MetricsCollector::new();
        collector.decrement_app_count();
        collector.decrement_app_count();
        assert_eq!(collector.collect().await.apps_count, 0);
    }

    #[tokio::test]
    async fn test_app_count_starts_at_zero() {
        let collector = MetricsCollector::new();
        assert_eq!(collector.collect().await.apps_count, 0);
    }

    #[test]
    fn test_system_metrics_serialization_roundtrip() {
        let m = SystemMetrics {
            cpu_usage: 0.42,
            ram_used_bytes: 1024,
            ram_total_bytes: 4096,
            disk_used_bytes: 500,
            disk_total_bytes: 1000,
            apps_count: 7,
            load_avg_1: 0.1,
            load_avg_5: 0.2,
            load_avg_15: 0.3,
            vms: HashMap::new(),
            timestamp: 1_700_000_000,
        };
        let json = serde_json::to_string(&m).unwrap();
        let restored: SystemMetrics = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.apps_count, 7);
        assert!((restored.cpu_usage - 0.42).abs() < 0.001);
        assert_eq!(restored.load_avg_1, 0.1);
        assert_eq!(restored.load_avg_5, 0.2);
        assert_eq!(restored.load_avg_15, 0.3);
        assert_eq!(restored.timestamp, 1_700_000_000);
    }

    #[tokio::test]
    async fn test_collector_is_cloneable() {
        let collector = MetricsCollector::new();
        collector.increment_app_count();
        let clone = collector.clone();
        // Cloned collector shares the same Arc state
        assert_eq!(clone.collect().await.apps_count, 1);
    }

    #[tokio::test]
    async fn test_collect_with_vms_and_metrics_file() {
        use crate::firecracker::config::FirecrackerConfig;
        use crate::firecracker::config::VmConfig;
        use crate::firecracker::manager::FirecrackerManager;

        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let collector = MetricsCollector::with_firecracker(mgr.clone());

        let vm_id = "test-vm-metrics";
        let metrics_file = format!(
            "{}/metrics-{}.json",
            mgr.fc_config.data_dir,
            uuid::Uuid::new_v4()
        );

        // Simulate Firecracker metrics JSON
        let metrics_content = serde_json::json!({
            "vcpu": { "exit_io_in": 10 },
            "balloon": { "inflate_count": 5 }
        })
        .to_string();
        tokio::fs::write(&metrics_file, metrics_content)
            .await
            .unwrap();

        // Start a stub VM
        mgr.start_vm(
            vm_id.to_string(),
            "app-1".to_string(),
            "image".to_string(),
            VmConfig::default(),
        )
        .await
        .unwrap();

        // Inject metrics path manually for testing since stub mode doesn't run the full background task
        {
            let mut processes = mgr.processes.lock().await;
            let log_task = tokio::spawn(async {});
            let child = tokio::process::Command::new("true").spawn().unwrap();
            processes.insert(
                vm_id.to_string(),
                crate::firecracker::process::VmProcess {
                    vm_id: vm_id.to_string(),
                    child,
                    socket_path: format!("{}/fake.sock", mgr.fc_config.data_dir),
                    metrics_path: Some(metrics_file.clone()),
                    tap_name: None,
                    log_task,
                    chroot_dir: None,
                },
            );
        }

        let metrics = collector.collect().await;
        assert!(metrics.vms.contains_key(vm_id));

        // Cleanup
        let _ = tokio::fs::remove_file(metrics_file).await;
    }
}
