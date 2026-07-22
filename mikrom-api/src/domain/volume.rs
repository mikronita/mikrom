use crate::domain::error::DomainResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, rovo::schemars::JsonSchema, Serialize, Deserialize)]
#[repr(i32)]
pub enum VolumeAccessMode {
    ReadWriteOnce = 0,
    ReadWriteMany = 1,
    ReadOnlyMany = 2,
}

impl VolumeAccessMode {
    pub fn as_i32(self) -> i32 {
        self as i32
    }

    pub fn is_read_only(self) -> bool {
        matches!(self, Self::ReadOnlyMany)
    }

    pub fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::ReadWriteOnce),
            1 => Some(Self::ReadWriteMany),
            2 => Some(Self::ReadOnlyMany),
            _ => None,
        }
    }
}

impl TryFrom<i32> for VolumeAccessMode {
    type Error = ();

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        Self::from_i32(value).ok_or(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, rovo::schemars::JsonSchema)]
pub struct Volume {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub size_mib: i32,
    #[serde(default)]
    pub pool_name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone, rovo::schemars::JsonSchema)]
pub struct AppVolume {
    pub app_id: Uuid,
    pub volume_id: Uuid,
    pub mount_point: String,
    pub access_mode: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone, rovo::schemars::JsonSchema)]
pub struct AttachedVolume {
    #[serde(flatten)]
    pub volume: Volume,
    pub mount_point: String,
    pub access_mode: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone, rovo::schemars::JsonSchema)]
pub struct VolumeAttachmentInfo {
    pub app_id: Uuid,
    pub app_name: String,
    pub mount_point: String,
    pub access_mode: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone, rovo::schemars::JsonSchema)]
pub struct VolumeWithAttachments {
    #[serde(flatten)]
    pub volume: Volume,
    pub attachments: Vec<VolumeAttachmentInfo>,
}

#[derive(Debug, Serialize, Deserialize, Clone, rovo::schemars::JsonSchema)]
pub struct VolumeSnapshot {
    pub id: Uuid,
    pub volume_id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug)]
pub struct CreateVolumeParams {
    pub user_id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub size_mib: i32,
    pub pool_name: String,
}

#[derive(Debug)]
pub struct CreateSnapshotParams {
    pub user_id: Uuid,
    pub volume_id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
}

#[mockall::automock]
#[async_trait]
pub trait VolumeRepository: Send + Sync {
    async fn create_volume(&self, params: CreateVolumeParams) -> DomainResult<Volume>;
    async fn get_volume(&self, volume_id: Uuid) -> DomainResult<Option<Volume>>;
    async fn list_volumes_by_tenant(
        &self,
        tenant_id: Uuid,
    ) -> DomainResult<Vec<VolumeWithAttachments>>;
    async fn delete_volume(&self, volume_id: Uuid) -> DomainResult<bool>;

    async fn attach_volume_to_app(
        &self,
        app_id: Uuid,
        volume_id: Uuid,
        mount_point: String,
        access_mode: i32,
    ) -> DomainResult<AppVolume>;
    async fn detach_volume_from_app(&self, app_id: Uuid, volume_id: Uuid) -> DomainResult<bool>;
    async fn list_volumes_by_app(&self, app_id: Uuid) -> DomainResult<Vec<AttachedVolume>>;
    async fn is_volume_attached(&self, volume_id: Uuid) -> DomainResult<bool>;

    async fn create_snapshot(&self, params: CreateSnapshotParams) -> DomainResult<VolumeSnapshot>;
    async fn get_snapshot(&self, snapshot_id: Uuid) -> DomainResult<Option<VolumeSnapshot>>;
    async fn list_snapshots_by_volume(&self, volume_id: Uuid) -> DomainResult<Vec<VolumeSnapshot>>;
    async fn delete_snapshot(&self, snapshot_id: Uuid) -> DomainResult<bool>;
}
