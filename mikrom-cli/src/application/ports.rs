use crate::domain::error::CliResult;
use crate::domain::models::*;
use async_trait::async_trait;
use mockall::automock;

#[automock]
#[async_trait]
pub trait ApiClient: Send + Sync {
    async fn health(&self) -> CliResult<HealthResponse>;
    async fn register(&self, email: &str, password: &str) -> CliResult<RegisterResponse>;
    async fn login(&self, email: &str, password: &str) -> CliResult<LoginResponse>;
    async fn whoami(&self) -> CliResult<WhoamiResponse>;
    async fn update_profile(
        &self,
        first_name: Option<String>,
        last_name: Option<String>,
    ) -> CliResult<WhoamiResponse>;

    async fn list_apps(&self) -> CliResult<Vec<AppInfo>>;
    async fn get_app(&self, app_name: &str) -> CliResult<AppInfo>;
    async fn create_app(&self, name: &str, git_url: &str) -> CliResult<AppInfo>;
    async fn delete_app(&self, app_id: &str) -> CliResult<()>;
    async fn get_app_secret(&self, app_name: &str) -> CliResult<Option<String>>;
    async fn deploy_app_version(
        &self,
        app_id: &str,
        vcpus: u32,
        memory_mib: u32,
        hypervisor: Option<String>,
    ) -> CliResult<DeployResponse>;
    async fn activate_deployment(&self, app_id: &str, deployment_id: &str) -> CliResult<()>;
    async fn list_app_deployments(&self, app_id: &str) -> CliResult<Vec<DeploymentInfo>>;

    async fn list_active_deployments(&self) -> CliResult<Vec<LiveDeploymentInfo>>;
    async fn get_deployment_status(
        &self,
        app_name: &str,
        job_id: &str,
    ) -> CliResult<LiveDeploymentStatus>;
    async fn stop_deployment(&self, app_name: &str, job_id: &str) -> CliResult<serde_json::Value>;
    async fn pause_deployment(&self, app_name: &str, job_id: &str) -> CliResult<serde_json::Value>;
    async fn resume_deployment(&self, app_name: &str, job_id: &str)
    -> CliResult<serde_json::Value>;
    async fn delete_deployment_record(
        &self,
        app_name: &str,
        job_id: &str,
    ) -> CliResult<serde_json::Value>;

    async fn scale_app(&self, app_id: &str, req: ScaleRequest) -> CliResult<()>;

    async fn list_volumes(&self, app_id: &str) -> CliResult<Vec<AttachedVolume>>;
    async fn list_all_volumes(&self) -> CliResult<Vec<VolumeWithAttachments>>;
    async fn create_volume(&self, name: &str, size_mib: i32) -> CliResult<Volume>;
    async fn attach_volume(
        &self,
        app_id: &str,
        volume_id: &str,
        mount_point: &str,
        access_mode: i32,
    ) -> CliResult<AppVolume>;
    async fn detach_volume(&self, app_id: &str, volume_id: &str) -> CliResult<()>;
    async fn create_volume_snapshot(
        &self,
        volume_id: &str,
        name: &str,
    ) -> CliResult<VolumeSnapshot>;
    async fn list_volume_snapshots(&self, volume_id: &str) -> CliResult<Vec<VolumeSnapshot>>;
    async fn restore_volume_snapshot(&self, volume_id: &str, snapshot_name: &str) -> CliResult<()>;
    async fn delete_volume_snapshot(&self, snapshot_id: &str) -> CliResult<()>;
    async fn delete_volume(&self, volume_id: &str) -> CliResult<()>;

    async fn list_databases(&self) -> CliResult<Vec<DatabaseInfo>>;
    async fn create_database(&self, req: CreateDatabaseRequest) -> CliResult<DatabaseInfo>;
    async fn delete_database(&self, db_id: &str) -> CliResult<()>;
    async fn get_database_connection_info(&self, db_id: &str) -> CliResult<DatabaseConnectionInfo>;
    async fn list_database_branches(&self, db_id: &str) -> CliResult<Vec<DatabaseBranchInfo>>;
    async fn get_database_backups(&self, db_id: &str) -> CliResult<DatabaseBackupInfo>;
    async fn list_database_snapshots(&self, db_id: &str)
    -> CliResult<DatabaseSnapshotListResponse>;
    async fn create_database_snapshot(
        &self,
        db_id: &str,
        name: &str,
    ) -> CliResult<DatabaseSnapshotActionResponse>;
    async fn restore_database_snapshot(
        &self,
        db_id: &str,
        snapshot_name: &str,
    ) -> CliResult<DatabaseSnapshotActionResponse>;
    async fn delete_database_snapshot(
        &self,
        db_id: &str,
        snapshot_name: &str,
    ) -> CliResult<DatabaseSnapshotActionResponse>;

    async fn list_projects(&self) -> CliResult<Vec<ProjectInfo>>;
    async fn create_project(&self, name: &str) -> CliResult<ProjectInfo>;

    async fn list_personal_access_tokens(&self) -> CliResult<Vec<PersonalAccessToken>>;
    async fn create_personal_access_token(&self, name: &str) -> CliResult<CreatedTokenResponse>;
    async fn revoke_personal_access_token(&self, token_id: &str) -> CliResult<()>;

    async fn list_user_notifications(
        &self,
        unread_only: bool,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> CliResult<NotificationListResponse>;
    async fn mark_user_notification_read(&self, notification_id: &str) -> CliResult<()>;
    async fn mark_all_user_notifications_read(&self) -> CliResult<()>;

    async fn list_vm_snapshots(
        &self,
        app_name: &str,
        job_id: &str,
    ) -> CliResult<DeploymentSnapshotListResponse>;
    async fn create_vm_snapshot(
        &self,
        app_name: &str,
        job_id: &str,
        name: &str,
    ) -> CliResult<DeploymentSnapshotActionResponse>;
    async fn restore_vm_snapshot(
        &self,
        app_name: &str,
        job_id: &str,
        snapshot_name: &str,
    ) -> CliResult<DeploymentSnapshotActionResponse>;
    async fn delete_vm_snapshot(
        &self,
        app_name: &str,
        job_id: &str,
        snapshot_name: &str,
    ) -> CliResult<DeploymentSnapshotActionResponse>;
}
