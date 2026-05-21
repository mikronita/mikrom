use crate::models::volume::{
    AppVolume, AttachedVolume, Volume, VolumeSnapshot, VolumeWithAttachments,
};
use async_trait::async_trait;
use uuid::Uuid;

#[derive(Debug)]
pub struct CreateVolumeParams {
    pub user_id: Uuid,
    pub name: String,
    pub size_mib: i32,
    pub pool_name: String,
}

#[derive(Debug)]
pub struct CreateSnapshotParams {
    pub volume_id: Uuid,
    pub user_id: Uuid,
    pub name: String,
}

#[mockall::automock]
#[async_trait]
pub trait VolumeRepository: Send + Sync {
    async fn create_volume(&self, params: CreateVolumeParams) -> anyhow::Result<Volume>;
    async fn get_volume(&self, volume_id: Uuid) -> anyhow::Result<Option<Volume>>;
    async fn list_volumes_by_user(
        &self,
        user_id: Uuid,
    ) -> anyhow::Result<Vec<VolumeWithAttachments>>;
    async fn delete_volume(&self, volume_id: Uuid) -> anyhow::Result<bool>;

    async fn attach_volume_to_app(
        &self,
        app_id: Uuid,
        volume_id: Uuid,
        mount_point: String,
        access_mode: i32,
    ) -> anyhow::Result<AppVolume>;
    async fn detach_volume_from_app(&self, app_id: Uuid, volume_id: Uuid) -> anyhow::Result<bool>;
    async fn list_volumes_by_app(&self, app_id: Uuid) -> anyhow::Result<Vec<AttachedVolume>>;
    async fn is_volume_attached(&self, volume_id: Uuid) -> anyhow::Result<bool>;

    async fn create_snapshot(&self, params: CreateSnapshotParams)
    -> anyhow::Result<VolumeSnapshot>;
    async fn get_snapshot(&self, snapshot_id: Uuid) -> anyhow::Result<Option<VolumeSnapshot>>;
    async fn list_snapshots_by_volume(
        &self,
        volume_id: Uuid,
    ) -> anyhow::Result<Vec<VolumeSnapshot>>;
    async fn delete_snapshot(&self, snapshot_id: Uuid) -> anyhow::Result<bool>;
}
