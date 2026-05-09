use crate::firecracker::{FirecrackerManager, VmStatus};
use mikrom_proto::id::{AppId, VmId};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use sysinfo::System;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VmMetrics {
    pub app_id: AppId,
    #[serde(default)]
    pub vm_id: VmId,
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
    pub status: VmStatus,
    pub error_message: Option<String>,
    pub ip_address: Option<String>,
    pub firecracker_metrics: Option<serde_json::Value>,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
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
    pub vms: HashMap<VmId, VmMetrics>,
    pub timestamp: i64,
}

impl Default for SystemMetrics {
    fn default() -> Self {
        let now = chrono::Utc::now().timestamp();
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
            timestamp: now,
        }
    }
}

pub struct MetricsCollector {
    sys: Arc<RwLock<System>>,
    firecracker: Option<FirecrackerManager>,
    apps_count: Arc<RwLock<u32>>,
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsCollector {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        Self {
            sys: Arc::new(RwLock::new(sys)),
            firecracker: None,
            apps_count: Arc::new(RwLock::new(0)),
        }
    }

    pub fn with_firecracker(firecracker: FirecrackerManager) -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        Self {
            sys: Arc::new(RwLock::new(sys)),
            firecracker: Some(firecracker),
            apps_count: Arc::new(RwLock::new(0)),
        }
    }

    pub fn increment_app_count(&self) {
        let mut count = self.apps_count.write();
        *count += 1;
    }

    pub fn decrement_app_count(&self) {
        let mut count = self.apps_count.write();
        if *count > 0 {
            *count -= 1;
        }
    }

    pub async fn collect(&self) -> SystemMetrics {
        let mut metrics = SystemMetrics::default();
        let now = chrono::Utc::now().timestamp();

        {
            let mut sys = self.sys.write();
            sys.refresh_all();

            metrics.cpu_usage = sys.global_cpu_usage();
            metrics.ram_used_bytes = sys.used_memory();
            metrics.ram_total_bytes = sys.total_memory();

            // Disk metrics (simplified)
            metrics.disk_used_bytes = 0;
            metrics.disk_total_bytes = 0;

            let load_avg = sysinfo::System::load_average();
            metrics.load_avg_1 = load_avg.one as f32;
            metrics.load_avg_5 = load_avg.five as f32;
            metrics.load_avg_15 = load_avg.fifteen as f32;

            metrics.apps_count = *self.apps_count.read();
            metrics.timestamp = now;
        }

        let vms_info = if let Some(mgr) = &self.firecracker {
            mgr.get_all_vms().await
        } else {
            Vec::new()
        };

        for vm in vms_info {
            let mut vm_metrics = VmMetrics {
                app_id: vm.app_id,
                vm_id: vm.vm_id,
                cpu_usage: 0.0,
                ram_used_bytes: 0,
                status: vm.status,
                error_message: vm.error_message,
                ip_address: vm.ip_address,
                firecracker_metrics: None,
                tx_bytes: 0,
                rx_bytes: 0,
            };

            // Attempt to read Firecracker metrics if path is available
            if let Some(metrics_path) = vm.metrics_path
                && let Ok(content) = tokio::fs::read_to_string(&metrics_path).await
                && let Ok(json) = serde_json::from_str::<serde_json::Value>(&content)
            {
                vm_metrics.firecracker_metrics = Some(json);
            }

            // Attempt to read eBPF stats if ifindex is available
            if let Some(ifindex) = vm.tap_ifindex
                && let Some(mgr) = &self.firecracker
            {
                let ebpf = mgr.ebpf_manager.lock().await;
                if let Some(ebpf) = ebpf.as_ref()
                    && let Some(stats) = ebpf.get_stats(ifindex)
                {
                    vm_metrics.tx_bytes = stats.tx_bytes;
                    vm_metrics.rx_bytes = stats.rx_bytes;
                }
            }

            metrics.vms.insert(vm_metrics.vm_id, vm_metrics);
        }

        metrics
    }
}

impl Clone for MetricsCollector {
    fn clone(&self) -> Self {
        Self {
            sys: self.sys.clone(),
            firecracker: self.firecracker.clone(),
            apps_count: self.apps_count.clone(),
        }
    }
}

pub struct FirecrackerExporter {
    client: async_nats::Client,
    collector: MetricsCollector,
    firecracker: FirecrackerManager,
}

impl FirecrackerExporter {
    pub fn new(
        client: async_nats::Client,
        collector: MetricsCollector,
        firecracker: FirecrackerManager,
    ) -> Self {
        Self {
            client,
            collector,
            firecracker,
        }
    }

    pub async fn start_export_loop(&self) {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
        loop {
            interval.tick().await;
            let metrics = self.collector.collect().await;

            // Publish host metrics
            let host_id = self.firecracker.agent_id.clone();
            let subject = format!("mikrom.telemetry.host.{}", host_id);

            if let Ok(payload) = serde_json::to_vec(&metrics)
                && let Err(e) = self.client.publish(subject, payload.into()).await
            {
                tracing::error!("Failed to publish metrics to NATS: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vm_metrics_serialization() {
        let app_id = AppId::new();
        let vm_id = VmId::new();
        let vm = VmMetrics {
            app_id,
            vm_id,
            cpu_usage: 0.5,
            ram_used_bytes: 1024,
            status: VmStatus::Running,
            error_message: None,
            ip_address: Some("10.0.0.1".to_string()),
            firecracker_metrics: None,
            tx_bytes: 100,
            rx_bytes: 200,
        };
        let json = serde_json::to_string(&vm).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["app_id"], app_id.to_string());
        assert_eq!(val["vm_id"], vm_id.to_string());
        assert_eq!(val["cpu_usage"], 0.5);
        assert_eq!(val["tx_bytes"], 100);
        assert_eq!(val["rx_bytes"], 200);
    }

    #[tokio::test]
    async fn test_system_metrics_serialization_roundtrip() {
        let mut metrics = SystemMetrics::default();
        let vm_id = VmId::new();
        metrics.vms.insert(
            vm_id,
            VmMetrics {
                app_id: AppId::new(),
                vm_id,
                cpu_usage: 0.1,
                ram_used_bytes: 512,
                status: VmStatus::Running,
                error_message: None,
                ip_address: None,
                firecracker_metrics: None,
                tx_bytes: 0,
                rx_bytes: 0,
            },
        );

        let json = serde_json::to_string(&metrics).unwrap();
        let restored: SystemMetrics = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.vms.len(), 1);
        assert!(restored.vms.contains_key(&vm_id));
    }

    #[test]
    fn test_system_metrics_default_zeroed() {
        let metrics = SystemMetrics::default();
        assert_eq!(metrics.cpu_usage, 0.0);
        assert_eq!(metrics.ram_used_bytes, 0);
        assert_eq!(metrics.apps_count, 0);
        assert!(metrics.vms.is_empty());
    }

    #[test]
    fn test_increment_app_count() {
        let collector = MetricsCollector::new();
        collector.increment_app_count();
        assert_eq!(*collector.apps_count.read(), 1);
    }

    #[test]
    fn test_decrement_app_count() {
        let collector = MetricsCollector::new();
        collector.increment_app_count();
        collector.decrement_app_count();
        assert_eq!(*collector.apps_count.read(), 0);
    }

    #[test]
    fn test_decrement_saturates_at_zero() {
        let collector = MetricsCollector::new();
        collector.decrement_app_count();
        assert_eq!(*collector.apps_count.read(), 0);
    }

    #[tokio::test]
    async fn test_collect_returns_real_system_data() {
        let collector = MetricsCollector::new();
        let metrics = collector.collect().await;
        assert!(metrics.ram_total_bytes > 0);
        assert!(metrics.timestamp > 0);
    }

    #[test]
    fn test_collector_is_cloneable() {
        let collector = MetricsCollector::new();
        let _cloned = collector.clone();
    }

    #[test]
    fn test_cpu_usage_within_valid_range() {
        let collector = MetricsCollector::new();
        let mut sys = collector.sys.write();
        sys.refresh_cpu_all();
        let usage = sys.global_cpu_usage();
        assert!((0.0..=100.0).contains(&usage));
    }

    #[test]
    fn test_ram_used_does_not_exceed_total() {
        let collector = MetricsCollector::new();
        let sys = collector.sys.read();
        assert!(sys.used_memory() <= sys.total_memory());
    }

    #[test]
    fn test_app_count_starts_at_zero() {
        let collector = MetricsCollector::new();
        assert_eq!(*collector.apps_count.read(), 0);
    }

    #[tokio::test]
    async fn test_collect_with_vms_and_metrics_file() {
        use crate::firecracker::config::FirecrackerConfig;
        use crate::firecracker::config::VmConfig;
        use crate::firecracker::manager::FirecrackerManager;

        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let collector = MetricsCollector::with_firecracker(mgr.clone());

        let vm_id = VmId::new();
        let app_id = AppId::new();
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
        mgr.start_vm(vm_id, app_id, "image".to_string(), VmConfig::default())
            .await
            .unwrap();

        // Inject metrics path manually for testing since stub mode doesn't run the full background task
        {
            let mut processes = mgr.processes.lock().await;
            let log_task = tokio::spawn(async {});
            let child = tokio::process::Command::new("true").spawn().unwrap();
            processes.insert(
                vm_id,
                crate::firecracker::process::VmProcess {
                    vm_id,
                    child,
                    socket_path: format!("{}/fake.sock", mgr.fc_config.data_dir),
                    metrics_path: Some(metrics_file.clone()),
                    tap_name: None,
                    tap_ifindex: None,
                    log_task,
                    chroot_dir: None,
                },
            );
        }

        let metrics = collector.collect().await;
        assert!(metrics.vms.contains_key(&vm_id));
        let vm_metrics = metrics.vms.get(&vm_id).unwrap();
        assert_eq!(vm_metrics.vm_id, vm_id);
        assert_eq!(vm_metrics.app_id, app_id);

        // Cleanup
        if let Err(e) = tokio::fs::remove_file(metrics_file).await {
            tracing::warn!("Failed to remove test metrics file: {}", e);
        }
    }
}
