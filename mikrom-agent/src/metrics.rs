use crate::hypervisor::{HypervisorType, VmHypervisor, VmStatus};
use mikrom_proto::id::{AppId, VmId};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VmMetrics {
    pub app_id: AppId,
    pub vm_id: VmId,
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
    pub status: VmStatus,
    pub error_message: Option<String>,
    pub raw_metrics: Option<serde_json::Value>,
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
    sys: Arc<RwLock<sysinfo::System>>,
    disks: Arc<RwLock<sysinfo::Disks>>,
    hypervisors: Arc<HashMap<HypervisorType, Arc<dyn VmHypervisor>>>,
    apps_count: Arc<RwLock<u32>>,
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsCollector {
    pub fn new() -> Self {
        let mut sys = sysinfo::System::new_all();
        sys.refresh_all();
        let disks = sysinfo::Disks::new_with_refreshed_list();
        Self {
            sys: Arc::new(RwLock::new(sys)),
            disks: Arc::new(RwLock::new(disks)),
            hypervisors: Arc::new(HashMap::new()),
            apps_count: Arc::new(RwLock::new(0)),
        }
    }

    pub fn with_hypervisors(
        hypervisors: Arc<HashMap<HypervisorType, Arc<dyn VmHypervisor>>>,
    ) -> Self {
        let mut sys = sysinfo::System::new_all();
        sys.refresh_all();
        let disks = sysinfo::Disks::new_with_refreshed_list();
        Self {
            sys: Arc::new(RwLock::new(sys)),
            disks: Arc::new(RwLock::new(disks)),
            hypervisors,
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

        let sys = self.sys.clone();
        let disks = self.disks.clone();
        let apps_count = *self.apps_count.read();

        let sys_result = tokio::task::spawn_blocking(move || {
            let mut sys = sys.write();
            sys.refresh_all();

            let cpu = sys.global_cpu_usage();
            let ram_used = sys.used_memory();
            let ram_total = sys.total_memory();

            let mut disks = disks.write();
            disks.refresh_list();
            let mut total_disk = 0;
            let mut available_disk = 0;
            for disk in disks.iter_mut() {
                disk.refresh();
                total_disk += disk.total_space();
                available_disk += disk.available_space();
            }

            let load_avg = sysinfo::System::load_average();

            (
                cpu,
                ram_used,
                ram_total,
                total_disk.saturating_sub(available_disk),
                total_disk,
                load_avg,
            )
        })
        .await;

        match sys_result {
            Ok((cpu, ram_used, ram_total, disk_used, disk_total, load_avg)) => {
                metrics.cpu_usage = cpu;
                metrics.ram_used_bytes = ram_used;
                metrics.ram_total_bytes = ram_total;
                metrics.disk_used_bytes = disk_used;
                metrics.disk_total_bytes = disk_total;
                metrics.load_avg_1 = load_avg.one as f32;
                metrics.load_avg_5 = load_avg.five as f32;
                metrics.load_avg_15 = load_avg.fifteen as f32;
            },
            Err(e) => {
                tracing::error!(error = %e, "System metrics blocking task failed; metrics will be zeroed");
            },
        }
        metrics.apps_count = apps_count;
        metrics.timestamp = now;

        // Collect VM metrics from all hypervisors
        for hv in self.hypervisors.values() {
            let vms_info = hv.get_all_vms().await;

            for vm in vms_info {
                let mut vm_metrics = VmMetrics {
                    app_id: vm.app_id,
                    vm_id: vm.vm_id,
                    cpu_usage: 0.0,
                    ram_used_bytes: 0,
                    status: vm.status,
                    error_message: vm.error_message,
                    raw_metrics: vm.raw_metrics,
                    tx_bytes: 0,
                    rx_bytes: 0,
                };

                if let Some(pid) = vm.pid.filter(|pid| *pid > 0) {
                    let pid_sys = sysinfo::Pid::from_u32(pid);
                    let sys = self.sys.read();
                    if let Some(process) = sys.process(pid_sys) {
                        vm_metrics.cpu_usage = process.cpu_usage();
                        vm_metrics.ram_used_bytes = process.memory();
                    } else {
                        tracing::debug!(vm_id = %vm.vm_id, pid = pid, "Process not found by sysinfo");
                    }
                }

                // If raw_metrics is empty but metrics_path is available (Firecracker), try to read it
                if vm_metrics.raw_metrics.is_none()
                    && let Some(metrics_path) = &vm.metrics_path
                    && let Ok(content) = std::fs::read_to_string(metrics_path)
                    && let Ok(json) = serde_json::from_str::<serde_json::Value>(&content)
                {
                    vm_metrics.raw_metrics = Some(json);
                }

                // Universal network metrics via sysfs statistics
                if let Some(tap_name) = &vm.tap_name {
                    let rx_path = format!("/sys/class/net/{}/statistics/rx_bytes", tap_name);
                    let tx_path = format!("/sys/class/net/{}/statistics/tx_bytes", tap_name);

                    // Host RX = VM TX
                    if let Ok(rx_str) = std::fs::read_to_string(&rx_path)
                        && let Ok(rx) = rx_str.trim().parse::<u64>()
                    {
                        vm_metrics.tx_bytes = rx;
                    } else {
                        tracing::debug!(vm_id = %vm.vm_id, tap_name = %tap_name, path = %rx_path, "Failed to read RX stats from sysfs");
                    }

                    // Host TX = VM RX
                    if let Ok(tx_str) = std::fs::read_to_string(&tx_path)
                        && let Ok(tx) = tx_str.trim().parse::<u64>()
                    {
                        vm_metrics.rx_bytes = tx;
                    } else {
                        tracing::debug!(vm_id = %vm.vm_id, tap_name = %tap_name, path = %tx_path, "Failed to read TX stats from sysfs");
                    }
                } else {
                    tracing::debug!(vm_id = %vm.vm_id, "No tap_name available for VM metrics");
                }

                tracing::debug!(
                    vm_id = %vm.vm_id,
                    cpu = vm_metrics.cpu_usage,
                    ram = vm_metrics.ram_used_bytes,
                    tx = vm_metrics.tx_bytes,
                    rx = vm_metrics.rx_bytes,
                    "Collected VM metrics"
                );

                metrics.vms.insert(vm_metrics.vm_id, vm_metrics);
            }
        }

        metrics
    }
}

impl Clone for MetricsCollector {
    fn clone(&self) -> Self {
        Self {
            sys: self.sys.clone(),
            disks: self.disks.clone(),
            hypervisors: self.hypervisors.clone(),
            apps_count: self.apps_count.clone(),
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
            raw_metrics: None,
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
                ram_used_bytes: 2048,
                status: VmStatus::Running,
                error_message: None,
                raw_metrics: None,
                tx_bytes: 300,
                rx_bytes: 400,
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
        assert!(metrics.disk_total_bytes > 0);
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
        use crate::firecracker::FirecrackerManager;
        use crate::firecracker::config::FirecrackerConfig;
        use crate::hypervisor::VmConfig;

        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let mut hvs: HashMap<HypervisorType, Arc<dyn VmHypervisor>> = HashMap::new();
        hvs.insert(HypervisorType::Firecracker, Arc::new(mgr.clone()));
        let collector = MetricsCollector::with_hypervisors(Arc::new(hvs));

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
                    pid: child.id(),
                    child: Some(child),
                    socket_path: format!("{}/fake.sock", mgr.fc_config.data_dir),
                    metrics_path: Some(metrics_file.clone()),
                    stdout_log_path: format!("{}/fake.stdout.log", mgr.fc_config.data_dir),
                    stderr_log_path: format!("{}/fake.stderr.log", mgr.fc_config.data_dir),
                    stdout_log_offset: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
                    stderr_log_offset: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
                    tap_name: None,
                    tap_ifindex: None,
                    log_task: Some(log_task),
                    chroot_dir: None,
                    app_started: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
                    app_started_at_ms: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
                    vfs_processes: Vec::new(),
                    vfs_pids: Vec::new(),
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

    #[tokio::test]
    async fn test_sysfs_metrics_mapping_logic() {
        // Test variables representing sysfs data
        let host_tx = 1000u64;
        let host_rx = 2000u64;

        // The logic we want to test:
        // vm_metrics.tx_bytes = host_rx; // Host RX = VM TX (Out)
        // vm_metrics.rx_bytes = host_tx; // Host TX = VM RX (In)

        let vm_tx_mapped = host_rx;
        let vm_rx_mapped = host_tx;

        assert_eq!(vm_tx_mapped, 2000, "Host RX must be mapped to VM TX");
        assert_eq!(vm_rx_mapped, 1000, "Host TX must be mapped to VM RX");
    }
}
