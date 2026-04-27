use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
#[repr(i32)]
pub enum JobStatus {
    #[default]
    Unspecified = 0,
    Pending = 1,
    Scheduled = 2,
    Running = 3,
    Failed = 4,
    Cancelled = 5,
    Stopped = 6,
}

impl From<i32> for JobStatus {
    fn from(code: i32) -> Self {
        match code {
            1 => JobStatus::Pending,
            2 => JobStatus::Scheduled,
            3 => JobStatus::Running,
            4 => JobStatus::Failed,
            5 => JobStatus::Cancelled,
            6 => JobStatus::Stopped,
            _ => JobStatus::Unspecified,
        }
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
    pub deployment_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Volume {
    pub volume_id: String,
    pub size_mib: u64,
    pub read_only: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct VmConfig {
    pub vcpus: u32,
    pub memory_mib: u64,
    pub disk_mib: u64,
    pub port: u32,
    pub env: std::collections::HashMap<String, String>,
    pub ip_address: Option<String>,
    pub gateway: Option<String>,
    pub mac_address: Option<String>,
    pub netmask: Option<String>,
    pub volumes: Vec<Volume>,
}

impl Job {
    #[must_use]
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
            deployment_id,
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

    pub fn stop(&mut self) {
        self.status = JobStatus::Stopped;
        self.stopped_at = Some(Utc::now().timestamp());
    }

    pub fn fail(&mut self, message: String) {
        self.status = JobStatus::Failed;
        self.error_message = Some(message);
        self.stopped_at = Some(Utc::now().timestamp());
    }

    pub fn cancel(&mut self) {
        self.status = JobStatus::Cancelled;
        self.stopped_at = Some(Utc::now().timestamp());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_status_default_is_pending() {
        assert_eq!(JobStatus::default(), JobStatus::Unspecified);
    }

    #[test]
    fn test_job_status_serialization_roundtrip() {
        let status = JobStatus::Running;
        let json = serde_json::to_string(&status).unwrap();
        let restored: JobStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, restored);
    }

    #[test]
    fn test_job_serialization_roundtrip() {
        let job = Job::new(
            "job-1".into(),
            "app-1".into(),
            "Hono App".into(),
            "nginx:latest".into(),
            VmConfig::default(),
            "user-1".into(),
            None,
        );
        let json = serde_json::to_string(&job).unwrap();
        let restored: Job = serde_json::from_str(&json).unwrap();
        assert_eq!(job.job_id, restored.job_id);
        assert_eq!(job.status, restored.status);
    }

    #[test]
    fn test_job_status_cast_to_i32() {
        // These values must match the proto definitions exactly
        assert_eq!(JobStatus::Pending as i32, 1);
        assert_eq!(JobStatus::Scheduled as i32, 2);
        assert_eq!(JobStatus::Running as i32, 3);
        assert_eq!(JobStatus::Failed as i32, 4);
        assert_eq!(JobStatus::Cancelled as i32, 5);
        assert_eq!(JobStatus::Stopped as i32, 6);
    }
}
