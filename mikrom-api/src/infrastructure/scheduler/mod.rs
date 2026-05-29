use crate::domain::error::DomainResult;
pub use crate::domain::scheduler::Scheduler;
use crate::nats::TypedNatsClient;
use mikrom_proto::scheduler::ListWorkersResponse;

pub struct NatsScheduler {
    nats: TypedNatsClient,
}

impl NatsScheduler {
    pub fn new(nats: TypedNatsClient) -> Self {
        Self { nats }
    }
}

#[async_trait::async_trait]
impl Scheduler for NatsScheduler {
    async fn pause_app(&self, job_id: String, user_id: String) -> DomainResult<bool> {
        let req = mikrom_proto::scheduler::PauseRequest { job_id, user_id };
        let res: mikrom_proto::scheduler::PauseResponse = self
            .nats
            .request(mikrom_proto::subjects::SCHEDULER_PAUSE_APP, req)
            .await?;
        Ok(res.success)
    }

    async fn resume_app(&self, job_id: String, user_id: String) -> DomainResult<bool> {
        let req = mikrom_proto::scheduler::ResumeRequest { job_id, user_id };
        let res: mikrom_proto::scheduler::ResumeResponse = self
            .nats
            .request(mikrom_proto::subjects::SCHEDULER_RESUME_APP, req)
            .await?;
        Ok(res.success)
    }

    async fn delete_app(&self, job_id: String, user_id: String) -> DomainResult<bool> {
        let req = mikrom_proto::scheduler::DeleteAppRequest { job_id, user_id };
        let res: mikrom_proto::scheduler::DeleteAppResponse = self
            .nats
            .request(mikrom_proto::subjects::SCHEDULER_DELETE_APP, req)
            .await?;
        Ok(res.success)
    }

    async fn delete_all_by_app(&self, app_id: String, user_id: String) -> DomainResult<bool> {
        let req = mikrom_proto::scheduler::DeleteAllByAppRequest { app_id, user_id };
        let res: mikrom_proto::scheduler::DeleteAllByAppResponse = self
            .nats
            .with_timeout(std::time::Duration::from_secs(15))
            .request("mikrom.scheduler.delete_all_by_app", req)
            .await?;
        Ok(res.success)
    }

    async fn scale_app(
        &self,
        app_id: String,
        desired_replicas: u32,
        user_id: String,
    ) -> DomainResult<bool> {
        let req = mikrom_proto::scheduler::ScaleAppRequest {
            app_id,
            desired_replicas,
            user_id,
        };
        let res: mikrom_proto::scheduler::ScaleAppResponse = self
            .nats
            .request(mikrom_proto::subjects::SCHEDULER_SCALE_APP, req)
            .await?;
        Ok(res.success)
    }

    async fn list_apps(
        &self,
        req: mikrom_proto::scheduler::ListAppsRequest,
    ) -> DomainResult<mikrom_proto::scheduler::ListAppsResponse> {
        let res: mikrom_proto::scheduler::ListAppsResponse = self
            .nats
            .request(mikrom_proto::subjects::SCHEDULER_LIST_APPS, req)
            .await?;
        Ok(res)
    }

    async fn update_app_scaling_config(
        &self,
        req: mikrom_proto::scheduler::UpdateAppScalingConfigRequest,
    ) -> DomainResult<bool> {
        let res: mikrom_proto::scheduler::UpdateAppScalingConfigResponse = self
            .nats
            .request(
                mikrom_proto::subjects::SCHEDULER_UPDATE_APP_SCALING_CONFIG,
                req,
            )
            .await?;
        Ok(res.success)
    }

    async fn list_workers(&self) -> DomainResult<ListWorkersResponse> {
        let req = mikrom_proto::scheduler::ListWorkersRequest {};
        let res: ListWorkersResponse = self
            .nats
            .request(mikrom_proto::subjects::SCHEDULER_LIST_WORKERS, req)
            .await?;
        Ok(res)
    }
}

#[must_use]
pub fn hypervisor_name(code: i32) -> String {
    use mikrom_proto::scheduler::HypervisorType;
    match HypervisorType::try_from(code).unwrap_or(HypervisorType::HypertypeUnspecified) {
        HypervisorType::HypertypeUnspecified => "firecracker",
        HypervisorType::HypertypeFirecracker => "firecracker",
        HypervisorType::HypertypeCloudHypervisor => "cloud-hypervisor",
    }
    .to_string()
}

#[must_use]
pub fn status_name(code: i32) -> &'static str {
    if code == 7 {
        return "STOPPED";
    }

    use mikrom_proto::scheduler::DeployStatus;
    match DeployStatus::try_from(code).unwrap_or(DeployStatus::Unspecified) {
        DeployStatus::Unspecified => "UNKNOWN",
        DeployStatus::Pending => "PENDING",
        DeployStatus::Scheduled => "SCHEDULED",
        DeployStatus::Running => "RUNNING",
        DeployStatus::Failed => "FAILED",
        DeployStatus::Cancelled => "CANCELLED",
        DeployStatus::Paused => "PAUSED",
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
        assert_eq!(status_name(6), "PAUSED");
        assert_eq!(status_name(7), "STOPPED");
    }

    #[test]
    fn test_status_name_unknown_code_returns_unspecified() {
        assert_eq!(status_name(99), "UNKNOWN");
        assert_eq!(status_name(-1), "UNKNOWN");
    }
}
