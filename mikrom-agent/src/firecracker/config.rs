use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FirecrackerError {
    #[error("VM not found: {0}")]
    VmNotFound(String),
    #[error("Failed to start VM: {0}")]
    StartFailed(String),
    #[error("Failed to stop VM: {0}")]
    StopFailed(String),
    #[error("Firecracker process error: {0}")]
    ProcessError(String),
    #[error("Firecracker API error on {path}: {msg}")]
    ApiError { path: String, msg: String },
    #[error("Timed out waiting for socket: {0}")]
    SocketTimeout(String),
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum VmStatus {
    Starting,
    Running,
    Paused,
    Stopping,
    #[default]
    Stopped,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VmInfo {
    pub vm_id: String,
    pub app_id: String,
    pub image: String,
    pub config: VmConfig,
    pub status: VmStatus,
    pub started_at: Option<i64>,
    pub error_message: Option<String>,
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

#[derive(Clone, Debug)]
pub struct FirecrackerConfig {
    pub kernel_path: Option<String>,
    pub binary: String,
    pub rootfs_path: String,
    pub data_dir: String,
    pub use_jailer: bool,
    pub jailer_binary: String,
    pub jailer_uid: u32,
    pub jailer_gid: u32,
    pub chroot_base: String,
}

impl FirecrackerConfig {
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            kernel_path: Some(
                std::env::var("FC_KERNEL_PATH")
                    .unwrap_or_else(|_| "/opt/firecracker/vmlinux.bin".to_string()),
            ),
            binary: std::env::var("FC_BINARY")
                .unwrap_or_else(|_| "/usr/bin/firecracker".to_string()),
            rootfs_path: std::env::var("FC_ROOTFS_PATH")
                .unwrap_or_else(|_| "/opt/firecracker/rootfs.ext4".to_string()),
            data_dir: std::env::var("FC_DATA_DIR")
                .unwrap_or_else(|_| "/var/lib/mikrom/data".to_string()),
            use_jailer: std::env::var("USE_JAILER").is_ok_and(|v| v == "true"),
            jailer_binary: std::env::var("JAILER_BINARY")
                .unwrap_or_else(|_| "/usr/bin/jailer".to_string()),
            jailer_uid: std::env::var("JAILER_UID")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1000),
            jailer_gid: std::env::var("JAILER_GID")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1000),
            chroot_base: std::env::var("JAILER_CHROOT_BASE")
                .unwrap_or_else(|_| "/srv/jailer".to_string()),
        }
    }

    #[must_use]
    pub fn stub() -> Self {
        Self {
            kernel_path: None,
            binary: String::new(),
            rootfs_path: String::new(),
            data_dir: "/tmp/mikrom-stub-data".to_string(),
            use_jailer: false,
            jailer_binary: String::new(),
            jailer_uid: 0,
            jailer_gid: 0,
            chroot_base: String::new(),
        }
    }
}
