use crate::firecracker::config::VmStatus;
use mikrom_proto::id::{AppId, VmId};

pub struct VmProcess {
    pub vm_id: VmId,
    pub child: tokio::process::Child,
    pub socket_path: String,
    pub metrics_path: Option<String>,
    pub tap_name: Option<String>,
    pub tap_ifindex: Option<u32>,
    pub log_task: tokio::task::JoinHandle<()>,
    pub chroot_dir: Option<String>,
}

/// Abstraction over shell command execution, allowing tests to inject a mock
/// instead of running real system commands (ip, mkfs, mount, etc.).
pub trait CommandExecutor: Send + Sync {
    fn name(&self) -> &'static str;
}

pub struct RealCommandExecutor;

impl CommandExecutor for RealCommandExecutor {
    fn name(&self) -> &'static str {
        "real"
    }
}

pub struct VmDetailedInfo {
    pub vm_id: VmId,
    pub app_id: AppId,
    pub status: VmStatus,
    pub error_message: Option<String>,
    pub pid: Option<u32>,
    pub ip_address: Option<String>,
    pub metrics_path: Option<String>,
    pub socket_path: Option<String>,
    pub tap_name: Option<String>,
    pub tap_ifindex: Option<u32>,
}
