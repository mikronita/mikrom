use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
#[repr(i32)]
pub enum JobStatus {
    #[default]
    Pending = 1,
    Scheduled = 2,
    Running = 3,
    Failed = 4,
    Cancelled = 5,
    Paused = 6,
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::get_unwrap)]
mod tests {
    use super::*;

    fn make_job(id: &str) -> Job {
        Job::new(
            id.to_string(),
            "app-1".to_string(),
            "my-app".to_string(),
            "nginx:latest".to_string(),
            VmConfig {
                vcpus: 1,
                memory_mib: 256,
                disk_mib: 1024,
                port: 8080,
                env: Default::default(),
                ip_address: None,
                gateway: None,
                mac_address: None,
                netmask: None,
                volumes: vec![],
            },
            "user-1".to_string(),
        )
    }

    #[test]
    fn test_new_job_is_pending() {
        let job = make_job("job-1");
        assert_eq!(job.status, JobStatus::Pending);
        assert!(job.host_id.is_none());
        assert!(job.vm_id.is_none());
        assert!(job.scheduled_at.is_none());
        assert!(job.started_at.is_none());
        assert!(job.stopped_at.is_none());
        assert!(job.error_message.is_none());
        assert!(job.created_at > 0);
    }

    #[test]
    fn test_schedule_sets_fields() {
        let mut job = make_job("job-2");
        job.schedule("host-1".to_string(), "vm-1".to_string());
        assert_eq!(job.status, JobStatus::Scheduled);
        assert_eq!(job.host_id.as_deref(), Some("host-1"));
        assert_eq!(job.vm_id.as_deref(), Some("vm-1"));
        assert!(job.scheduled_at.is_some());
    }

    #[test]
    fn test_start_sets_fields() {
        let mut job = make_job("job-3");
        job.schedule("host-1".to_string(), "vm-1".to_string());
        job.start();
        assert_eq!(job.status, JobStatus::Running);
        assert!(job.started_at.is_some());
    }

    #[test]
    fn test_fail_sets_fields() {
        let mut job = make_job("job-4");
        job.fail("out of memory".to_string());
        assert_eq!(job.status, JobStatus::Failed);
        assert_eq!(job.error_message.as_deref(), Some("out of memory"));
        assert!(job.stopped_at.is_some());
    }

    #[test]
    fn test_cancel_sets_fields() {
        let mut job = make_job("job-5");
        job.cancel();
        assert_eq!(job.status, JobStatus::Cancelled);
        assert!(job.stopped_at.is_some());
    }

    #[test]
    fn test_job_status_default_is_pending() {
        assert_eq!(JobStatus::default(), JobStatus::Pending);
    }

    #[test]
    fn test_vmconfig_default() {
        let config = VmConfig::default();
        assert_eq!(config.vcpus, 0);
        assert_eq!(config.memory_mib, 0);
        assert_eq!(config.disk_mib, 0);
        assert!(config.env.is_empty());
        assert!(config.ip_address.is_none());
        assert!(config.gateway.is_none());
        assert!(config.mac_address.is_none());
        assert!(config.volumes.is_empty());
    }

    #[test]
    fn test_job_serialization_roundtrip() {
        let job = make_job("job-ser");
        let json = serde_json::to_string(&job).unwrap();
        let restored: Job = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.job_id, "job-ser");
        assert_eq!(restored.app_id, "app-1");
        assert_eq!(restored.image, "nginx:latest");
        assert_eq!(restored.status, JobStatus::Pending);
        assert_eq!(restored.user_id, "user-1");
    }

    #[test]
    fn test_job_status_serialization() {
        assert_eq!(
            serde_json::to_string(&JobStatus::Pending).unwrap(),
            "\"pending\""
        );
        assert_eq!(
            serde_json::to_string(&JobStatus::Scheduled).unwrap(),
            "\"scheduled\""
        );
        assert_eq!(
            serde_json::to_string(&JobStatus::Running).unwrap(),
            "\"running\""
        );
        assert_eq!(
            serde_json::to_string(&JobStatus::Failed).unwrap(),
            "\"failed\""
        );
        assert_eq!(
            serde_json::to_string(&JobStatus::Cancelled).unwrap(),
            "\"cancelled\""
        );
    }

    #[test]
    fn test_job_status_deserialization() {
        let s: JobStatus = serde_json::from_str("\"scheduled\"").unwrap();
        assert_eq!(s, JobStatus::Scheduled);
    }

    #[test]
    fn test_vmconfig_with_env() {
        let mut env = std::collections::HashMap::new();
        env.insert("PORT".to_string(), "8080".to_string());
        let config = VmConfig {
            vcpus: 2,
            memory_mib: 512,
            disk_mib: 2048,
            port: 8080,
            env,
            ip_address: None,
            gateway: None,
            mac_address: None,
            netmask: None,
            volumes: vec![],
        };
        assert_eq!(&config.env["PORT"], "8080");
    }

    #[test]
    fn test_job_status_cast_to_i32() {
        // Values must match proto DeployStatus (0 = Unspecified, so ours start at 1).
        assert_eq!(JobStatus::Pending as i32, 1);
        assert_eq!(JobStatus::Scheduled as i32, 2);
        assert_eq!(JobStatus::Running as i32, 3);
        assert_eq!(JobStatus::Failed as i32, 4);
        assert_eq!(JobStatus::Cancelled as i32, 5);
    }
}
