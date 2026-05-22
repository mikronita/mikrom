use mikrom_proto::id::VmId;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64},
};

#[derive(Debug)]
pub struct VmProcess {
    pub vm_id: VmId,
    pub child: Option<tokio::process::Child>,
    pub pid: Option<u32>,
    pub socket_path: String,
    pub metrics_path: Option<String>,
    pub tap_name: Option<String>,
    pub tap_ifindex: Option<u32>,
    pub log_task: Option<tokio::task::JoinHandle<()>>,
    pub stdout_log_path: String,
    pub stderr_log_path: String,
    pub stdout_log_offset: Arc<AtomicU64>,
    pub stderr_log_offset: Arc<AtomicU64>,
    pub chroot_dir: Option<String>,
    pub app_started: Arc<AtomicBool>,
    pub app_started_at_ms: Arc<AtomicU64>,
    pub vfs_processes: Vec<tokio::process::Child>,
    pub vfs_pids: Vec<u32>,
}
