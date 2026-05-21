use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, rovo::schemars::JsonSchema)]
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

#[derive(Debug, Serialize, Deserialize, FromRow, Clone, rovo::schemars::JsonSchema)]
pub struct Volume {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub size_mib: i32,
    #[serde(skip_serializing)]
    pub pool_name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone, rovo::schemars::JsonSchema)]
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

#[derive(Debug, Serialize, Deserialize, FromRow, Clone, rovo::schemars::JsonSchema)]
pub struct VolumeSnapshot {
    pub id: Uuid,
    pub volume_id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample_volume() -> Volume {
        Volume {
            id: Uuid::from_u128(1),
            user_id: Uuid::from_u128(2),
            name: "data".to_string(),
            size_mib: 1024,
            pool_name: "pool-a".to_string(),
            created_at: Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
            updated_at: Utc.timestamp_opt(1_700_000_100, 0).unwrap(),
        }
    }

    #[test]
    fn volume_serialization_hides_pool_name() {
        let json = serde_json::to_value(sample_volume()).unwrap();
        assert_eq!(json["name"], "data");
        assert_eq!(json["size_mib"], 1024);
        assert_eq!(json["pool_name"], serde_json::Value::Null);
        assert_eq!(json["id"], Uuid::from_u128(1).to_string());
    }

    #[test]
    fn attached_volume_serializes_flat_volume_fields() {
        let attached = AttachedVolume {
            volume: sample_volume(),
            mount_point: "/data".to_string(),
            access_mode: 2,
        };

        let json = serde_json::to_value(attached).unwrap();
        assert_eq!(json["id"], Uuid::from_u128(1).to_string());
        assert_eq!(json["mount_point"], "/data");
        assert_eq!(json["access_mode"], 2);
    }

    #[test]
    fn volume_with_attachments_serializes_attachment_list() {
        let value = VolumeWithAttachments {
            volume: sample_volume(),
            attachments: vec![VolumeAttachmentInfo {
                app_id: Uuid::from_u128(3),
                app_name: "svc".to_string(),
                mount_point: "/data".to_string(),
                access_mode: 1,
            }],
        };

        let json = serde_json::to_value(value).unwrap();
        assert_eq!(json["id"], Uuid::from_u128(1).to_string());
        assert_eq!(json["attachments"][0]["app_name"], "svc");
        assert_eq!(json["attachments"][0]["access_mode"], 1);
    }

    #[test]
    fn app_volume_serializes_expected_shape() {
        let value = AppVolume {
            app_id: Uuid::from_u128(4),
            volume_id: Uuid::from_u128(5),
            mount_point: "/mnt/data".to_string(),
            access_mode: 0,
            created_at: Utc.timestamp_opt(1_700_000_200, 0).unwrap(),
        };

        let json = serde_json::to_value(value).unwrap();
        assert_eq!(json["app_id"], Uuid::from_u128(4).to_string());
        assert_eq!(json["volume_id"], Uuid::from_u128(5).to_string());
        assert_eq!(json["mount_point"], "/mnt/data");
    }

    #[test]
    fn volume_access_mode_converts_from_i32() {
        assert_eq!(
            VolumeAccessMode::from_i32(0).unwrap(),
            VolumeAccessMode::ReadWriteOnce
        );
        assert_eq!(
            VolumeAccessMode::from_i32(1).unwrap(),
            VolumeAccessMode::ReadWriteMany
        );
        assert_eq!(
            VolumeAccessMode::from_i32(2).unwrap(),
            VolumeAccessMode::ReadOnlyMany
        );
        assert!(VolumeAccessMode::from_i32(99).is_none());
    }

    #[test]
    fn volume_access_mode_flags_read_only() {
        assert!(VolumeAccessMode::ReadOnlyMany.is_read_only());
        assert!(!VolumeAccessMode::ReadWriteOnce.is_read_only());
        assert_eq!(VolumeAccessMode::ReadWriteMany.as_i32(), 1);
    }
}
