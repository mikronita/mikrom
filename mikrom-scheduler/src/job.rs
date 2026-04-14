use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    Scheduled,
    Running,
    Failed,
    Cancelled,
}

impl Default for JobStatus {
    fn default() -> Self {
        Self::Pending
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub job_id: String,
    pub app_id: String,
    pub app_name: String,
    pub image: String,
    pub config: VmConfig,
    pub user_id: String,
    pub status: JobStatus,
    pub host_id: Option<String>,
    pub vm_id: Option<String>,
    pub scheduled_at: Option<i64>,
    pub started_at: Option<i64>,
    pub stopped_at: Option<i64>,
    pub error_message: Option<String>,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct VmConfig {
    pub vcpus: u32,
    pub memory_mib: u64,
    pub disk_mib: u64,
    pub env: std::collections::HashMap<String, String>,
}

impl Job {
    pub fn new(
        job_id: String,
        app_id: String,
        app_name: String,
        image: String,
        config: VmConfig,
        user_id: String,
    ) -> Self {
        Self {
            job_id,
            app_id,
            app_name,
            image,
            config,
            user_id,
            status: JobStatus::Pending,
            host_id: None,
            vm_id: None,
            scheduled_at: None,
            started_at: None,
            stopped_at: None,
            error_message: None,
            created_at: Utc::now().timestamp(),
        }
    }

    pub fn schedule(&mut self, host_id: String, vm_id: String) {
        self.status = JobStatus::Scheduled;
        self.host_id = Some(host_id);
        self.vm_id = Some(vm_id);
        self.scheduled_at = Some(Utc::now().timestamp());
    }

    pub fn start(&mut self) {
        self.status = JobStatus::Running;
        self.started_at = Some(Utc::now().timestamp());
    }

    pub fn fail(&mut self, error: String) {
        self.status = JobStatus::Failed;
        self.error_message = Some(error);
        self.stopped_at = Some(Utc::now().timestamp());
    }

    pub fn cancel(&mut self) {
        self.status = JobStatus::Cancelled;
        self.stopped_at = Some(Utc::now().timestamp());
    }
}
