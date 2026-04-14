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
