use crate::domain::error::DomainResult;
use mikrom_proto::scheduler::ListWorkersResponse;

#[mockall::automock]
#[async_trait::async_trait]
pub trait Scheduler: Send + Sync {
    async fn pause_app(&self, job_id: String, user_id: String) -> DomainResult<bool>;
    async fn resume_app(&self, job_id: String, user_id: String) -> DomainResult<bool>;
    async fn delete_app(&self, job_id: String, user_id: String) -> DomainResult<bool>;
    async fn delete_all_by_app(&self, app_id: String, user_id: String) -> DomainResult<bool>;
    async fn scale_app(
        &self,
        app_id: String,
        desired_replicas: u32,
        user_id: String,
    ) -> DomainResult<bool>;
    async fn list_apps(
        &self,
        req: mikrom_proto::scheduler::ListAppsRequest,
    ) -> DomainResult<mikrom_proto::scheduler::ListAppsResponse>;
    async fn update_app_scaling_config(
        &self,
        req: mikrom_proto::scheduler::UpdateAppScalingConfigRequest,
    ) -> DomainResult<bool>;
    async fn list_workers(&self) -> DomainResult<ListWorkersResponse>;
}
