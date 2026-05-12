use crate::firecracker::process::VmProcess;
use mikrom_proto::id::VmId;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64},
};
use tokio::process::Child;
use tokio::task::JoinHandle;

/// RAII guard to ensure resources are cleaned up if VM startup fails.
pub struct VmStartupGuard {
    pub vm_id: VmId,
    pub child: Option<Child>,
    pub tap_name: Option<String>,
    pub tap_ifindex: Option<u32>,
    pub chroot_dir: Option<PathBuf>,
    pub log_task: Option<JoinHandle<()>>,
    pub app_started: Arc<AtomicBool>,
    pub app_started_at_ms: Arc<AtomicU64>,
    pub socket_path: PathBuf,
    pub metrics_path: Option<String>,
    committed: bool,
}

impl VmStartupGuard {
    pub fn new(vm_id: VmId, socket_path: PathBuf) -> Self {
        Self {
            vm_id,
            child: None,
            tap_name: None,
            tap_ifindex: None,
            chroot_dir: None,
            log_task: None,
            app_started: Arc::new(AtomicBool::new(false)),
            app_started_at_ms: Arc::new(AtomicU64::new(0)),
            socket_path,
            metrics_path: None,
            committed: false,
        }
    }

    /// Mark the startup as successful, preventing automatic cleanup on Drop.
    pub fn commit(mut self) -> VmProcess {
        self.committed = true;
        VmProcess {
            vm_id: self.vm_id,
            child: self
                .child
                .take()
                .expect("Child process must exist at commit"),
            socket_path: self.socket_path.to_string_lossy().to_string(),
            metrics_path: self.metrics_path.take(),
            tap_name: self.tap_name.take(),
            tap_ifindex: self.tap_ifindex,
            log_task: self.log_task.take().expect("Log task must exist at commit"),
            chroot_dir: self
                .chroot_dir
                .take()
                .map(|p| p.to_string_lossy().to_string()),
            app_started: self.app_started.clone(),
            app_started_at_ms: self.app_started_at_ms.clone(),
        }
    }
}

impl Drop for VmStartupGuard {
    fn drop(&mut self) {
        if !self.committed {
            let vm_id = self.vm_id;
            let child = self.child.take();
            let tap_name = self.tap_name.take();
            let chroot_dir = self.chroot_dir.take();
            let log_task = self.log_task.take();

            tokio::spawn(async move {
                tracing::warn!(vm_id = %vm_id, "Startup failed or guard dropped, cleaning up resources...");

                if let Some(mut c) = child {
                    let _ = c.kill().await;
                }

                if let Some(handle) = log_task {
                    handle.abort();
                }

                if let Some(chroot) = chroot_dir {
                    #[allow(clippy::collapsible_if)]
                    if let Err(e) = tokio::fs::remove_dir_all(&chroot).await {
                        tracing::error!(vm_id = %vm_id, path = ?chroot, error = %e, "Failed to cleanup chroot directory");
                    }
                }

                // Note: cleanup_tap is not easily accessible here without a manager reference.
                // We might need to handle tap cleanup separately or pass a cleanup closure.
                if let Some(tap) = tap_name {
                    tracing::warn!(vm_id = %vm_id, tap = %tap, "Tap cleanup skipped in guard drop (needs manual cleanup or manager reference)");
                }
            });
        }
    }
}
