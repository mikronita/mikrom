use crate::domain::error::DomainResult;
use crate::domain::job::{Job, JobStatus, VmConfig};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmMetrics {
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostMetrics {
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
    pub ram_total_bytes: u64,
    pub disk_used_bytes: u64,
    pub disk_total_bytes: u64,
    pub apps_count: u32,
    pub load_avg_1: f32,
    pub load_avg_5: f32,
    pub load_avg_15: f32,
    pub timestamp: i64,
    pub vms: HashMap<String, VmMetrics>,
}

impl HostMetrics {
    pub fn can_fit_vm(&self, memory_mib: u64, disk_mib: u64) -> bool {
        let ram_free = self.ram_total_bytes.saturating_sub(self.ram_used_bytes);
        let disk_free = self.disk_total_bytes.saturating_sub(self.disk_used_bytes);

        // Convert MiB to bytes
        let ram_req = memory_mib * 1024 * 1024;
        let disk_req = disk_mib * 1024 * 1024;

        let ram_ok = ram_free >= ram_req;
        let disk_ok = if self.disk_total_bytes == 0 {
            // Metrics not implemented or no limit reported, bypass for resilience
            true
        } else {
            disk_free >= disk_req
        };

        ram_ok && disk_ok
    }

    pub fn calculate_score(&self, max_apps: u32) -> f32 {
        let cpu_score = 1.0 - (self.cpu_usage / 100.0);
        let ram_score = 1.0 - (self.ram_used_bytes as f32 / self.ram_total_bytes as f32);
        let apps_score = 1.0 - (self.apps_count as f32 / max_apps as f32);

        (cpu_score * 0.4 + ram_score * 0.4 + apps_score * 0.2).max(0.0)
    }
}

#[derive(Debug, Clone)]
pub struct Worker {
    pub host_id: String,
    pub hostname: String,
    pub ip_address: String,
    pub bridge_ip: String,
    pub wireguard_pubkey: Option<String>,
    pub wireguard_ip: Option<String>,
    pub wireguard_port: Option<i32>,
    pub metrics: Option<HostMetrics>,
    pub registered_at: i64,
    pub last_heartbeat: i64,
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub enum SchedulingStrategy {
    #[default]
    LeastLoaded,
    BinPacking,
}

#[mockall::automock]
#[async_trait]
pub trait WorkerRepository: Send + Sync {
    async fn register(&self, worker: Worker) -> DomainResult<()>;
    async fn unregister(&self, host_id: &str) -> DomainResult<()>;
    async fn update_metrics(&self, host_id: &str, metrics: HostMetrics) -> DomainResult<()>;
    async fn get_worker(&self, host_id: &str) -> DomainResult<Option<Worker>>;
    async fn list_workers(&self) -> DomainResult<Vec<Worker>>;
    async fn get_available_workers(&self, threshold_secs: i64) -> DomainResult<Vec<Worker>>;
}

#[mockall::automock]
#[async_trait]
pub trait AgentClient: Send + Sync {
    async fn start_vm(
        &self,
        host_id: &str,
        app_id: &str,
        image: &str,
        vm_id: &str,
        config: &VmConfig,
    ) -> DomainResult<()>;
    async fn pause_vm(&self, host_id: &str, vm_id: &str) -> DomainResult<()>;
    async fn resume_vm(&self, host_id: &str, vm_id: &str) -> DomainResult<()>;
    async fn stop_vm(&self, host_id: &str, vm_id: &str) -> DomainResult<()>;
    async fn delete_vm(&self, host_id: &str, vm_id: &str) -> DomainResult<()>;
    async fn check_health(&self, host_id: &str, vm_id: &str) -> DomainResult<bool>;
    async fn update_firewall(
        &self,
        host_id: &str,
        vm_id: &str,
        rules: Vec<mikrom_proto::scheduler::FirewallRule>,
    ) -> DomainResult<()>;
}

#[mockall::automock]
#[async_trait]
pub trait JobRepository: Send + Sync {
    async fn add_job(&self, job: Job) -> DomainResult<()>;
    async fn get_job(&self, job_id: &str) -> DomainResult<Option<Job>>;
    async fn update_job_status(&self, job_id: &str, status: JobStatus) -> DomainResult<()>;
    async fn start_job(&self, job_id: &str, timestamp: i64) -> DomainResult<()>;
    async fn fail_job(&self, job_id: &str, message: String, timestamp: i64) -> DomainResult<()>;
    async fn update_job_ip(
        &self,
        job_id: &str,
        ip: &str,
        gateway: &str,
        mac: &str,
        netmask: &str,
    ) -> DomainResult<()>;
    async fn cancel_job(&self, job_id: &str, timestamp: i64) -> DomainResult<()>;
    async fn remove_job(&self, job_id: &str) -> DomainResult<()>;
    async fn remove_jobs_by_app(&self, app_id: &str) -> DomainResult<()>;
    async fn list_jobs<'a>(
        &self,
        user_id: Option<&'a str>,
        app_id: Option<&'a str>,
        status: Option<JobStatus>,
    ) -> DomainResult<Vec<Job>>;
    async fn find_job_by_vm_id(&self, vm_id: &str) -> DomainResult<Option<Job>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_fit_vm_standard() {
        let metrics = HostMetrics {
            cpu_usage: 0.0,
            ram_total_bytes: 2048 * 1024 * 1024,
            ram_used_bytes: 512 * 1024 * 1024,
            disk_total_bytes: 10 * 1024 * 1024 * 1024,
            disk_used_bytes: 2 * 1024 * 1024 * 1024,
            apps_count: 0,
            load_avg_1: 0.0,
            load_avg_5: 0.0,
            load_avg_15: 0.0,
            timestamp: 0,
            vms: HashMap::new(),
        };

        // Fits: 512MB RAM, 1GB Disk
        assert!(metrics.can_fit_vm(512, 1024));
        // Does not fit: 2GB RAM
        assert!(!metrics.can_fit_vm(2048, 1024));
        // Does not fit: 9GB Disk
        assert!(!metrics.can_fit_vm(512, 9 * 1024));
    }

    #[test]
    fn test_can_fit_vm_resilience_bypass() {
        let metrics = HostMetrics {
            cpu_usage: 0.0,
            ram_total_bytes: 2048 * 1024 * 1024,
            ram_used_bytes: 512 * 1024 * 1024,
            disk_total_bytes: 0, // 0 means metrics not implemented
            disk_used_bytes: 0,
            apps_count: 0,
            load_avg_1: 0.0,
            load_avg_5: 0.0,
            load_avg_15: 0.0,
            timestamp: 0,
            vms: HashMap::new(),
        };

        // Fits because disk check is bypassed
        assert!(metrics.can_fit_vm(512, 1024));
        // Still fails RAM check
        assert!(!metrics.can_fit_vm(2048, 1024));
    }
}
