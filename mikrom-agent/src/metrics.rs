use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use sysinfo::System;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SystemMetrics {
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
    pub ram_total_bytes: u64,
    pub disk_used_bytes: u64,
    pub disk_total_bytes: u64,
    pub apps_count: u32,
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
            timestamp: chrono::Utc::now().timestamp(),
        }
    }
}

#[derive(Clone)]
pub struct MetricsCollector {
    system: Arc<RwLock<System>>,
    apps_count: Arc<RwLock<u32>>,
}

impl MetricsCollector {
    pub fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_all();

        Self {
            system: Arc::new(RwLock::new(system)),
            apps_count: Arc::new(RwLock::new(0)),
        }
    }

    pub fn collect(&self) -> SystemMetrics {
        let mut system = self.system.write();
        system.refresh_all();

        let cpu_usage = system.global_cpu_usage() / 100.0;
        let ram_used_bytes = system.used_memory();
        let ram_total_bytes = system.total_memory();

        let (disk_used_bytes, disk_total_bytes) = self.get_disk_usage();

        let apps_count = *self.apps_count.read();

        SystemMetrics {
            cpu_usage,
            ram_used_bytes,
            ram_total_bytes,
            disk_used_bytes,
            disk_total_bytes,
            apps_count,
            timestamp: chrono::Utc::now().timestamp(),
        }
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

    #[test]
    fn test_collect_returns_real_system_data() {
        let collector = MetricsCollector::new();
        let metrics = collector.collect();
        assert!(metrics.ram_total_bytes > 0, "total RAM must be > 0");
        assert!(metrics.timestamp > 0);
    }

    #[test]
    fn test_cpu_usage_within_valid_range() {
        let collector = MetricsCollector::new();
        let metrics = collector.collect();
        assert!(metrics.cpu_usage >= 0.0);
        assert!(metrics.cpu_usage <= 1.0);
    }

    #[test]
    fn test_ram_used_does_not_exceed_total() {
        let collector = MetricsCollector::new();
        let metrics = collector.collect();
        assert!(metrics.ram_used_bytes <= metrics.ram_total_bytes);
    }

    #[test]
    fn test_increment_app_count() {
        let collector = MetricsCollector::new();
        collector.increment_app_count();
        collector.increment_app_count();
        assert_eq!(collector.collect().apps_count, 2);
    }

    #[test]
    fn test_decrement_app_count() {
        let collector = MetricsCollector::new();
        collector.increment_app_count();
        collector.increment_app_count();
        collector.decrement_app_count();
        assert_eq!(collector.collect().apps_count, 1);
    }

    #[test]
    fn test_decrement_saturates_at_zero() {
        let collector = MetricsCollector::new();
        collector.decrement_app_count();
        collector.decrement_app_count();
        assert_eq!(collector.collect().apps_count, 0);
    }

    #[test]
    fn test_app_count_starts_at_zero() {
        let collector = MetricsCollector::new();
        assert_eq!(collector.collect().apps_count, 0);
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
            timestamp: 1_700_000_000,
        };
        let json = serde_json::to_string(&m).unwrap();
        let restored: SystemMetrics = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.apps_count, 7);
        assert!((restored.cpu_usage - 0.42).abs() < 0.001);
        assert_eq!(restored.timestamp, 1_700_000_000);
    }

    #[test]
    fn test_collector_is_cloneable() {
        let collector = MetricsCollector::new();
        collector.increment_app_count();
        let clone = collector.clone();
        // Cloned collector shares the same Arc state
        assert_eq!(clone.collect().apps_count, 1);
    }
}
