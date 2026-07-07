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
    pub stdout_log_path: String,
    pub stderr_log_path: String,
    pub stdout_log_offset: Arc<AtomicU64>,
    pub stderr_log_offset: Arc<AtomicU64>,
    pub app_started: Arc<AtomicBool>,
    pub app_started_at_ms: Arc<AtomicU64>,
    pub socket_path: PathBuf,
    pub metrics_path: Option<String>,
    pub vfs_processes: Vec<Child>,
    pub vfs_pids: Vec<u32>,
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
            stdout_log_path: String::new(),
            stderr_log_path: String::new(),
            stdout_log_offset: Arc::new(AtomicU64::new(0)),
            stderr_log_offset: Arc::new(AtomicU64::new(0)),
            app_started: Arc::new(AtomicBool::new(false)),
            app_started_at_ms: Arc::new(AtomicU64::new(0)),
            socket_path,
            metrics_path: None,
            vfs_processes: Vec::new(),
            vfs_pids: Vec::new(),
            committed: false,
        }
    }

    /// Mark the startup as successful, preventing automatic cleanup on Drop.
    /// Returns `None` if the child process or log task are missing (startup
    /// was never fully initialised).
    pub fn commit(mut self) -> Option<VmProcess> {
        self.committed = true;
        let child = self.child.take()?;
        let pid = child.id();
        let log_task = self.log_task.take()?;
        Some(VmProcess {
            vm_id: self.vm_id,
            child: Some(child),
            pid,
            socket_path: self.socket_path.to_string_lossy().to_string(),
            metrics_path: self.metrics_path.take(),
            tap_name: self.tap_name.take(),
            tap_ifindex: self.tap_ifindex,
            log_task: Some(log_task),
            stdout_log_path: self.stdout_log_path.clone(),
            stderr_log_path: self.stderr_log_path.clone(),
            stdout_log_offset: self.stdout_log_offset.clone(),
            stderr_log_offset: self.stderr_log_offset.clone(),
            chroot_dir: self
                .chroot_dir
                .take()
                .map(|p| p.to_string_lossy().to_string()),
            app_started: self.app_started.clone(),
            app_started_at_ms: self.app_started_at_ms.clone(),
            vfs_pids: self.vfs_pids.clone(),
            vfs_processes: std::mem::take(&mut self.vfs_processes),
        })
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
            let vfs_processes = std::mem::take(&mut self.vfs_processes);

            tokio::spawn(async move {
                tracing::warn!(vm_id = %vm_id, "Startup failed or guard dropped, cleaning up resources...");

                for mut vfs in vfs_processes {
                    let _ = vfs.kill().await;
                }

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

                if let Some(tap) = tap_name {
                    use futures::stream::TryStreamExt;
                    let (connection, handle, _) = match rtnetlink::new_connection() {
                        Ok(c) => c,
                        Err(e) => {
                            tracing::error!(vm_id = %vm_id, tap = %tap, error = %e, "Failed to create netlink connection for TAP cleanup");
                            return;
                        }
                    };
                    tokio::spawn(connection);
                    let mut links = handle.link().get().match_name(tap.clone()).execute();
                    if let Ok(Some(msg)) = links.try_next().await {
                        let _ = handle.link().set(msg.header.index).nocontroller().execute().await;
                        let _ = handle.link().del(msg.header.index).execute().await;
                    }
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[tokio::test]
    async fn test_commit_preserves_child_pid_and_state() {
        let vm_id = VmId::new();
        let mut guard = VmStartupGuard::new(vm_id, PathBuf::from("/run/firecracker.socket"));
        guard.log_task = Some(tokio::spawn(async {}));

        let child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("sleep 1")
            .spawn()
            .expect("failed to spawn child");
        let child_pid = child.id();
        guard.child = Some(child);
        guard.tap_name = Some("m-tap-test".to_string());
        guard.tap_ifindex = Some(42);
        guard.chroot_dir = Some(PathBuf::from("/tmp/test-chroot"));
        guard.metrics_path = Some("/metrics.json".to_string());
        guard.stdout_log_path = "/tmp/stdout.log".to_string();
        guard.stderr_log_path = "/tmp/stderr.log".to_string();
        guard.vfs_pids = vec![11, 22];
        guard.app_started.store(true, Ordering::SeqCst);
        guard.app_started_at_ms.store(1234, Ordering::SeqCst);

        let mut process = guard
            .commit()
            .expect("commit should succeed when child and log_task are set");
        assert_eq!(process.pid, child_pid);
        assert!(process.child.is_some());
        assert_eq!(process.socket_path, "/run/firecracker.socket");
        assert_eq!(process.tap_name.as_deref(), Some("m-tap-test"));
        assert_eq!(process.tap_ifindex, Some(42));
        assert_eq!(process.metrics_path.as_deref(), Some("/metrics.json"));
        assert_eq!(process.stdout_log_path, "/tmp/stdout.log");
        assert_eq!(process.stderr_log_path, "/tmp/stderr.log");
        assert_eq!(process.vfs_pids, vec![11, 22]);
        assert_eq!(process.chroot_dir.as_deref(), Some("/tmp/test-chroot"));
        assert!(process.app_started.load(Ordering::SeqCst));
        assert_eq!(process.app_started_at_ms.load(Ordering::SeqCst), 1234);

        if let Some(child) = process.child.as_mut() {
            let _ = child.kill().await;
        }
    }
}
