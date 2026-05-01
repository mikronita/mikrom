use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, Copy)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    #[default]
    Pending,
    Scheduled,
    Running,
    Stopped,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Volume {
    pub volume_id: String,
    pub size_mib: u64,
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmConfig {
    pub vcpus: u32,
    pub memory_mib: u64,
    pub disk_mib: u64,
    pub port: u32,
    pub env: HashMap<String, String>,
    pub ip_address: Option<String>,
    pub gateway: Option<String>,
    pub mac_address: Option<String>,
    pub netmask: Option<String>,
    pub volumes: Vec<Volume>,
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            vcpus: 1,
            memory_mib: 128,
            disk_mib: 512,
            port: 8080,
            env: HashMap::new(),
            ip_address: None,
            gateway: None,
            mac_address: None,
            netmask: None,
            volumes: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub job_id: String,
    pub app_id: String,
    pub app_name: String,
    pub image: String,
    pub user_id: String,
    pub status: JobStatus,
    pub host_id: Option<String>,
    pub vm_id: Option<String>,
    pub scheduled_at: Option<i64>,
    pub started_at: Option<i64>,
    pub stopped_at: Option<i64>,
    pub error_message: Option<String>,
    pub created_at: i64,
    pub deployment_id: Option<String>,
    pub config: VmConfig,
}

impl Job {
    pub fn new(
        job_id: String,
        app_id: String,
        app_name: String,
        image: String,
        config: VmConfig,
        user_id: String,
        deployment_id: Option<String>,
    ) -> Self {
        Self {
            job_id,
            app_id,
            app_name,
            image,
            user_id,
            status: JobStatus::Pending,
            host_id: None,
            vm_id: None,
            scheduled_at: None,
            started_at: None,
            stopped_at: None,
            error_message: None,
            created_at: chrono::Utc::now().timestamp(),
            deployment_id,
            config,
        }
    }

    pub fn schedule(&mut self, host_id: String, vm_id: String) {
        self.host_id = Some(host_id);
        self.vm_id = Some(vm_id);
        self.status = JobStatus::Scheduled;
        self.scheduled_at = Some(chrono::Utc::now().timestamp());
    }
}
