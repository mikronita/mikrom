use crate::models::volume::{Volume, VolumeSnapshot};
use async_trait::async_trait;
use uuid::Uuid;

#[derive(Debug)]
pub struct CreateVolumeParams {
    pub app_id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub size_mib: i32,
    pub pool_name: String,
    pub mount_point: String,
    pub access_mode: i32,
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
    async fn list_volumes_by_app(&self, app_id: Uuid) -> anyhow::Result<Vec<Volume>>;
    async fn delete_volume(&self, volume_id: Uuid) -> anyhow::Result<bool>;

    async fn create_snapshot(&self, params: CreateSnapshotParams)
    -> anyhow::Result<VolumeSnapshot>;
    async fn get_snapshot(&self, snapshot_id: Uuid) -> anyhow::Result<Option<VolumeSnapshot>>;
    async fn list_snapshots_by_volume(
        &self,
        volume_id: Uuid,
    ) -> anyhow::Result<Vec<VolumeSnapshot>>;
    async fn delete_snapshot(&self, snapshot_id: Uuid) -> anyhow::Result<bool>;
}
