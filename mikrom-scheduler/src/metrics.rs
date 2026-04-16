use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HostMetrics {
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
    pub ram_total_bytes: u64,
    pub disk_used_bytes: u64,
    pub disk_total_bytes: u64,
    pub apps_count: u32,
    pub timestamp: i64,
}

impl HostMetrics {
    pub fn calculate_score(&self, max_apps: u32) -> f32 {
        let cpu_score = 1.0 - self.cpu_usage;
        let ram_score = if self.ram_total_bytes > 0 {
            1.0 - (self.ram_used_bytes as f32 / self.ram_total_bytes as f32)
        } else {
            0.0
        };
        let disk_score = if self.disk_total_bytes > 0 {
            1.0 - (self.disk_used_bytes as f32 / self.disk_total_bytes as f32)
        } else {
            0.0
        };
        let apps_score = if max_apps > 0 {
            1.0 - (self.apps_count as f32 / max_apps as f32)
        } else {
            1.0
        };

        (cpu_score * 0.25) + (ram_score * 0.25) + (disk_score * 0.25) + (apps_score * 0.25)
    }

    pub fn can_fit_vm(&self, required_memory_mib: u64, required_disk_mib: u64) -> bool {
        let available_ram_mib =
            self.ram_total_bytes.saturating_sub(self.ram_used_bytes) / (1024 * 1024);
        let available_disk_mib =
            self.disk_total_bytes.saturating_sub(self.disk_used_bytes) / (1024 * 1024);

        let disk_check = if self.disk_total_bytes == 0 {
            true
        } else {
            required_disk_mib <= available_disk_mib
        };

        required_memory_mib <= available_ram_mib && disk_check
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics(
        cpu: f32,
        ram_used: u64,
        ram_total: u64,
        disk_used: u64,
        disk_total: u64,
        apps: u32,
    ) -> HostMetrics {
        HostMetrics {
            cpu_usage: cpu,
            ram_used_bytes: ram_used,
            ram_total_bytes: ram_total,
            disk_used_bytes: disk_used,
            disk_total_bytes: disk_total,
            apps_count: apps,
            timestamp: 0,
        }
    }

    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;

    #[test]
    fn test_score_idle_host_is_one() {
        // cpu=0, ram 0/4G, disk 0/100G, apps 0/10 → all sub-scores 1.0
        let m = metrics(0.0, 0, 4 * GIB, 0, 100 * GIB, 0);
        let score = m.calculate_score(10);
        assert!((score - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_score_fully_saturated_is_zero() {
        // cpu=1, ram used=total, disk used=total, apps=max
        let m = metrics(1.0, 4 * GIB, 4 * GIB, 100 * GIB, 100 * GIB, 10);
        let score = m.calculate_score(10);
        assert!(score.abs() < 0.001);
    }

    #[test]
    fn test_score_half_loaded() {
        // each dimension at 50 % → each sub-score 0.5 → total 0.5
        let m = metrics(0.5, 2 * GIB, 4 * GIB, 50 * GIB, 100 * GIB, 5);
        let score = m.calculate_score(10);
        assert!((score - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_score_zero_total_ram_gives_zero_ram_component() {
        // ram_total = 0 → ram_score = 0; disk_total = 0 → disk_score = 0
        // cpu_score = 1.0, apps_score = 1.0  → (0.25 + 0 + 0 + 0.25) = 0.5
        let m = metrics(0.0, 0, 0, 0, 0, 0);
        let score = m.calculate_score(10);
        assert!((score - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_score_zero_max_apps_gives_full_apps_score() {
        // max_apps = 0 → apps_score = 1.0
        let m = metrics(0.0, 0, 4 * GIB, 0, 100 * GIB, 99);
        let score = m.calculate_score(0);
        assert!((score - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_score_weights_sum_to_one() {
        // Verify formula: all four weights are 0.25 each
        // Set each component to a known fraction and check the arithmetic
        // cpu=0.8 → cpu_score=0.2; ram 3/4 used → 0.25; disk 70/100 used → 0.3; apps 6/10 → 0.4
        let m = metrics(0.8, 3 * GIB, 4 * GIB, 70 * GIB, 100 * GIB, 6);
        let expected = (0.2 + 0.25 + 0.3 + 0.4) * 0.25;
        let score = m.calculate_score(10);
        assert!((score - expected).abs() < 0.01);
    }

    #[test]
    fn test_can_fit_vm_success() {
        // 4 GiB total, 1 GiB used → 3072 MiB available; disk 100 GiB, 0 used
        let m = metrics(0.0, GIB, 4 * GIB, 0, 100 * GIB, 0);
        assert!(m.can_fit_vm(2048, 50 * 1024));
    }

    #[test]
    fn test_can_fit_vm_exact_boundary() {
        // exactly 3072 MiB RAM available, requesting 3072
        let m = metrics(0.0, GIB, 4 * GIB, 0, 100 * GIB, 0);
        assert!(m.can_fit_vm(3072, 0));
    }

    #[test]
    fn test_can_fit_vm_not_enough_ram() {
        // 4 GiB total, 3.5 GiB used → 512 MiB available
        let m = metrics(0.0, 3584 * MIB, 4 * GIB, 0, 100 * GIB, 0);
        assert!(!m.can_fit_vm(1024, 0));
    }

    #[test]
    fn test_can_fit_vm_not_enough_disk() {
        // disk: 100 GiB total, 90 GiB used → 10240 MiB available; requesting 20480 MiB
        let m = metrics(0.0, 0, 4 * GIB, 90 * GIB, 100 * GIB, 0);
        assert!(!m.can_fit_vm(256, 20 * 1024));
    }

    #[test]
    fn test_can_fit_vm_zero_disk_total_skips_disk_check() {
        // disk_total = 0 → disk check always passes
        let m = metrics(0.0, 0, 4 * GIB, 0, 0, 0);
        assert!(m.can_fit_vm(1024, 999_999));
    }

    #[test]
    fn test_host_metrics_default() {
        let m = HostMetrics::default();
        assert_eq!(m.cpu_usage, 0.0);
        assert_eq!(m.apps_count, 0);
        assert_eq!(m.ram_total_bytes, 0);
    }

    #[test]
    fn test_host_metrics_serialization_roundtrip() {
        let m = metrics(0.5, 2 * GIB, 4 * GIB, 10 * GIB, 100 * GIB, 3);
        let json = serde_json::to_string(&m).unwrap();
        let restored: HostMetrics = serde_json::from_str(&json).unwrap();
        assert!((restored.cpu_usage - 0.5).abs() < 0.001);
        assert_eq!(restored.apps_count, 3);
    }
}
