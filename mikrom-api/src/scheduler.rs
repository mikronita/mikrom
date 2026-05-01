#[async_trait::async_trait]
pub trait Scheduler: Send + Sync {
    async fn pause_app(&self, job_id: String, user_id: String) -> Result<bool, String>;
    async fn resume_app(&self, job_id: String, user_id: String) -> Result<bool, String>;
    async fn delete_app(&self, job_id: String, user_id: String) -> Result<bool, String>;
}

pub struct NatsScheduler {
    pub client: async_nats::Client,
}

#[async_trait::async_trait]
impl Scheduler for NatsScheduler {
    async fn pause_app(&self, job_id: String, user_id: String) -> Result<bool, String> {
        use mikrom_proto::scheduler::{PauseRequest, PauseResponse};
        use prost::Message;
        let nats_req = PauseRequest { job_id, user_id };
        let mut buf = Vec::new();
        nats_req.encode(&mut buf).map_err(|e| e.to_string())?;

        let response = self
            .client
            .request(mikrom_proto::subjects::SCHEDULER_PAUSE_APP, buf.into())
            .await
            .map_err(|e| e.to_string())?;

        let inner = PauseResponse::decode(&response.payload[..]).map_err(|e| e.to_string())?;

        Ok(inner.success)
    }

    async fn resume_app(&self, job_id: String, user_id: String) -> Result<bool, String> {
        use mikrom_proto::scheduler::{ResumeRequest, ResumeResponse};
        use prost::Message;
        let nats_req = ResumeRequest { job_id, user_id };
        let mut buf = Vec::new();
        nats_req.encode(&mut buf).map_err(|e| e.to_string())?;

        let response = self
            .client
            .request(mikrom_proto::subjects::SCHEDULER_RESUME_APP, buf.into())
            .await
            .map_err(|e| e.to_string())?;

        let inner = ResumeResponse::decode(&response.payload[..]).map_err(|e| e.to_string())?;

        Ok(inner.success)
    }

    async fn delete_app(&self, job_id: String, user_id: String) -> Result<bool, String> {
        use mikrom_proto::scheduler::{DeleteAppRequest, DeleteAppResponse};
        use prost::Message;
        let nats_req = DeleteAppRequest { job_id, user_id };
        let mut buf = Vec::new();
        nats_req.encode(&mut buf).map_err(|e| e.to_string())?;

        let response = self
            .client
            .request(mikrom_proto::subjects::SCHEDULER_DELETE_APP, buf.into())
            .await
            .map_err(|e| e.to_string())?;

        let inner = DeleteAppResponse::decode(&response.payload[..]).map_err(|e| e.to_string())?;

        Ok(inner.success)
    }
}

mockall::mock! {
    pub Scheduler {}
    #[async_trait::async_trait]
    impl Scheduler for Scheduler {
        async fn pause_app(&self, job_id: String, user_id: String) -> Result<bool, String>;
        async fn resume_app(&self, job_id: String, user_id: String) -> Result<bool, String>;
        async fn delete_app(&self, job_id: String, user_id: String) -> Result<bool, String>;
    }
}

#[must_use]
pub fn status_name(code: i32) -> &'static str {
    use mikrom_proto::scheduler::DeployStatus;
    match DeployStatus::try_from(code).unwrap_or(DeployStatus::Unspecified) {
        DeployStatus::Unspecified => "UNKNOWN",
        DeployStatus::Pending => "PENDING",
        DeployStatus::Scheduled => "SCHEDULED",
        DeployStatus::Running => "RUNNING",
        DeployStatus::Failed => "FAILED",
        DeployStatus::Cancelled => "CANCELLED",
        DeployStatus::Paused => "STOPPED",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_name_all_variants() {
        assert_eq!(status_name(0), "UNKNOWN");
        assert_eq!(status_name(1), "PENDING");
        assert_eq!(status_name(2), "SCHEDULED");
        assert_eq!(status_name(3), "RUNNING");
        assert_eq!(status_name(4), "FAILED");
        assert_eq!(status_name(5), "CANCELLED");
        assert_eq!(status_name(6), "STOPPED");
    }

    #[test]
    fn test_status_name_unknown_code_returns_unspecified() {
        assert_eq!(status_name(99), "UNKNOWN");
        assert_eq!(status_name(-1), "UNKNOWN");
    }
}
