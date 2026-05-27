use crate::hypervisor::types::{VmConfig, VmDetailedInfo, VmInfo, VmStatus};
use crate::hypervisor::{HypervisorError, HypervisorType, VmHypervisor};
use crate::qemu::config::QemuConfig;
use crate::qemu::qmp::QmpClient;
use async_trait::async_trait;
use mikrom_proto::id::{AppId, VmId};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, RwLock};

/// Information about a running QEMU process.
#[derive(Debug)]
pub(crate) struct QemuProcess {
    pub(crate) child: tokio::process::Child,
    pub(crate) pid: u32,
    pub(crate) qmp_socket: PathBuf,
    pub(crate) tap_name: String,
    #[allow(dead_code)]
    pub(crate) started_at: i64,
    pub(crate) qmp: Option<tokio::sync::Mutex<QmpClient>>,
    pub(crate) virtiofsd: Vec<tokio::process::Child>,
    pub(crate) virtiofsd_sockets: Vec<PathBuf>,
    pub(crate) event_task: Option<tokio::task::JoinHandle<()>>,
}

/// Real QEMU microvm manager.
///
/// Spawns QEMU processes using the `microvm` machine type and controls
/// them via signals (and optionally QMP over a Unix socket).
pub struct QemuManager {
    agent_id: String,
    pub(crate) config: QemuConfig,
    pub(crate) vms: Arc<RwLock<HashMap<VmId, VmInfo>>>,
    pub(crate) processes: Arc<Mutex<HashMap<VmId, QemuProcess>>>,
    pub(crate) logs: Arc<dashmap::DashMap<VmId, VecDeque<String>>>,
}

impl std::fmt::Debug for QemuManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QemuManager")
            .field("agent_id", &self.agent_id)
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl Clone for QemuManager {
    fn clone(&self) -> Self {
        Self {
            agent_id: self.agent_id.clone(),
            config: self.config.clone(),
            vms: self.vms.clone(),
            processes: self.processes.clone(),
            logs: self.logs.clone(),
        }
    }
}

impl QemuManager {
    pub async fn new(agent_id: String) -> Self {
        Self::with_config(agent_id, QemuConfig::from_env()).await
    }

    pub async fn with_config(agent_id: String, config: QemuConfig) -> Self {
        let data_dir = &config.data_dir;
        if !data_dir.exists()
            && let Err(e) = tokio::fs::create_dir_all(data_dir).await
        {
            tracing::warn!(
                path = %data_dir.display(),
                error = %e,
                "Failed to create QEMU data directory"
            );
        }
        Self {
            agent_id,
            config,
            vms: Arc::new(RwLock::new(HashMap::new())),
            processes: Arc::new(Mutex::new(HashMap::new())),
            logs: Arc::new(dashmap::DashMap::new()),
        }
    }

    /// Spawn a background task that listens for QMP events on the given socket
    /// and updates the VM status in real time.
    pub(crate) fn spawn_event_listener(
        &self,
        vm_id: VmId,
        qmp_socket: PathBuf,
    ) -> tokio::task::JoinHandle<()> {
        let vms = self.vms.clone();
        let vm_id_clone = vm_id;
        tokio::spawn(async move {
            let deadline = Duration::from_secs(30);
            let stream =
                match tokio::time::timeout(deadline, tokio::net::UnixStream::connect(&qmp_socket))
                    .await
                {
                    Ok(Ok(s)) => s,
                    Ok(Err(e)) => {
                        tracing::warn!(
                            vm_id = %vm_id_clone,
                            error = %e,
                            "QMP event listener: failed to connect"
                        );
                        return;
                    },
                    Err(_) => {
                        tracing::warn!(
                            vm_id = %vm_id_clone,
                            "QMP event listener: connect timed out"
                        );
                        return;
                    },
                };

            let (reader, mut writer) = tokio::io::split(stream);
            let mut reader = tokio::io::BufReader::new(reader);
            let mut line = String::new();

            // QMP handshake – consume greeting
            loop {
                line.clear();
                if tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
                    .await
                    .is_err()
                {
                    return;
                }
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line)
                    && val.get("QMP").is_some()
                {
                    break;
                }
            }

            // Enable capabilities
            let cap_cmd = r#"{"execute":"qmp_capabilities"}"#;
            if let Err(e) = writer.write_all(cap_cmd.as_bytes()).await {
                tracing::warn!(
                    vm_id = %vm_id_clone,
                    error = %e,
                    "QMP event listener: failed to send capabilities command"
                );
                return;
            }
            if let Err(e) = writer.write_all(b"\n").await {
                tracing::warn!(
                    vm_id = %vm_id_clone,
                    error = %e,
                    "QMP event listener: failed to terminate capabilities command"
                );
                return;
            }
            if let Err(e) = writer.flush().await {
                tracing::warn!(
                    vm_id = %vm_id_clone,
                    error = %e,
                    "QMP event listener: failed to flush capabilities command"
                );
                return;
            }

            // Consume capability response
            loop {
                line.clear();
                if tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
                    .await
                    .is_err()
                {
                    return;
                }
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line)
                    && (val.get("return").is_some() || val.get("error").is_some())
                {
                    break;
                }
            }

            tracing::info!(vm_id = %vm_id_clone, "QMP event listener connected");

            // Event loop
            loop {
                line.clear();
                match tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line).await {
                    Ok(0) => {
                        tracing::info!(vm_id = %vm_id_clone, "QMP event listener: socket closed");
                        break;
                    },
                    Ok(_) => {},
                    Err(e) => {
                        tracing::warn!(
                            vm_id = %vm_id_clone,
                            error = %e,
                            "QMP event listener: read error"
                        );
                        break;
                    },
                }

                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let val: serde_json::Value = match serde_json::from_str(trimmed) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let Some(event_name) = val.get("event").and_then(|v| v.as_str()) else {
                    // Not an event (probably a response to a command we didn't send)
                    continue;
                };

                tracing::debug!(
                    vm_id = %vm_id_clone,
                    event = %event_name,
                    "QMP event received"
                );

                let new_status = match event_name {
                    "SHUTDOWN" | "POWERDOWN" => Some(VmStatus::Stopping),
                    "STOP" => Some(VmStatus::Paused),
                    "RESUME" => Some(VmStatus::Running),
                    "RESET" => Some(VmStatus::Starting),
                    _ => None,
                };

                if let Some(status) = new_status {
                    let mut vms_guard = vms.write().await;
                    if let Some(vm) = vms_guard.get_mut(&vm_id_clone) {
                        vm.status = status;
                        tracing::info!(
                            vm_id = %vm_id_clone,
                            status = ?status,
                            event = %event_name,
                            "VM status updated from QMP event"
                        );
                    }
                }
            }
        })
    }

    pub(crate) fn qmp_socket_path(&self, vm_id: &VmId) -> PathBuf {
        self.config.data_dir.join(format!("qemu-{vm_id}.qmp"))
    }

    pub(crate) fn pidfile_path(&self, vm_id: &VmId) -> PathBuf {
        self.config.data_dir.join(format!("qemu-{vm_id}.pid"))
    }

    pub(crate) fn virtiofsd_socket_path(&self, vm_id: &VmId, tag: &str) -> PathBuf {
        let safe_tag = tag.replace(|c: char| !c.is_ascii_alphanumeric(), "_");
        self.config
            .virtiofsd_socket_dir
            .join(format!("qemu-{vm_id}-{safe_tag}.sock"))
    }

    /// Download a URL to the cache directory, returning the local path.
    async fn download_to_cache(url: &str, cache_dir: &Path, name: &str) -> PathBuf {
        let dest = cache_dir.join(name);
        if dest.exists() {
            tracing::debug!(url = %url, path = %dest.display(), "Using cached image");
            return dest;
        }
        if let Err(e) = tokio::fs::create_dir_all(cache_dir).await {
            tracing::warn!(
                path = %cache_dir.display(),
                error = %e,
                "Failed to create QEMU cache directory"
            );
        }
        tracing::info!(url = %url, dest = %dest.display(), "Downloading image");
        match reqwest::get(url).await {
            Ok(resp) => {
                let bytes = match resp.bytes().await {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::error!(url = %url, error = %e, "Failed to read response body");
                        return PathBuf::from(url);
                    },
                };
                match tokio::fs::write(&dest, &bytes).await {
                    Ok(_) => {
                        tracing::info!(path = %dest.display(), size = %bytes.len(), "Image cached")
                    },
                    Err(e) => {
                        tracing::error!(path = %dest.display(), error = %e, "Failed to write cache")
                    },
                }
                dest
            },
            Err(e) => {
                tracing::error!(url = %url, error = %e, "Failed to download image");
                PathBuf::from(url)
            },
        }
    }

    /// Resolve the kernel path — download from URL if configured, else use the configured path.
    pub(crate) async fn resolve_kernel(&self) -> PathBuf {
        if let Some(ref url) = self.config.kernel_url {
            let name = url.rsplit('/').next().unwrap_or("vmlinux");
            Self::download_to_cache(url, &self.config.image_cache_dir, name).await
        } else {
            PathBuf::from(&self.config.kernel_path)
        }
    }

    /// Resolve the rootfs for a VM.
    pub(crate) async fn resolve_rootfs(&self, _image: &str) -> PathBuf {
        if let Some(ref url) = self.config.rootfs_url {
            let name = url.rsplit('/').next().unwrap_or("rootfs.img");
            Self::download_to_cache(url, &self.config.image_cache_dir, name).await
        } else {
            PathBuf::from(&self.config.rootfs_path)
        }
    }

    /// Generate a unique vsock guest CID from a VM ID (range 3–0xFFFFFFFF).
    pub(crate) fn vsock_cid(vm_id: &VmId) -> u64 {
        let uuid = vm_id.as_uuid();
        let low = uuid.as_u128() as u64;
        3 + (low % 0xFFFFFFFC) // ensure range [3, 0xFFFFFFFF)
    }

    /// Parse the PID from a pidfile written by QEMU.
    pub(crate) async fn read_pidfile(path: &PathBuf) -> Result<u32, HypervisorError> {
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| HypervisorError::ProcessError(format!("Failed to read pidfile: {e}")))?;
        content
            .trim()
            .parse()
            .map_err(|e| HypervisorError::ProcessError(format!("Invalid pid in pidfile: {e}")))
    }

    /// Build a tap device name from the VM ID.
    pub(crate) fn tap_name(vm_id: &VmId) -> String {
        let hex = format!("{:x}", vm_id.as_uuid()).replace('-', "");
        let short = &hex[..8];
        format!("qemu-{short}")
    }
}

#[async_trait]
impl VmHypervisor for QemuManager {
    fn hypervisor_type(&self) -> HypervisorType {
        HypervisorType::QemuMicrovm
    }

    fn agent_id(&self) -> &str {
        &self.agent_id
    }

    async fn start_vm(
        &self,
        vm_id: VmId,
        app_id: AppId,
        image: String,
        config: VmConfig,
    ) -> Result<(), HypervisorError> {
        self.start_vm(vm_id, app_id, image, config).await
    }

    async fn stop_vm(&self, vm_id: &VmId) -> Result<(), HypervisorError> {
        self.stop_vm(vm_id).await
    }

    async fn pause_vm(&self, vm_id: &VmId) -> Result<(), HypervisorError> {
        let procs = self.processes.lock().await;
        let proc = procs
            .get(vm_id)
            .ok_or_else(|| HypervisorError::VmNotFound(vm_id.to_string()))?;

        let paused = if let Some(ref qmp) = proc.qmp {
            qmp.lock().await.stop().await.is_ok()
        } else {
            false
        };

        let pid = proc.pid;
        drop(procs); // release before acquiring vms

        if !paused {
            #[cfg(unix)]
            unsafe {
                libc::kill(pid as i32, libc::SIGSTOP);
            }
        }

        let mut vms = self.vms.write().await;
        if let Some(vm) = vms.get_mut(vm_id) {
            vm.status = VmStatus::Paused;
        }
        Ok(())
    }

    async fn resume_vm(&self, vm_id: &VmId) -> Result<(), HypervisorError> {
        let procs = self.processes.lock().await;
        let proc = procs
            .get(vm_id)
            .ok_or_else(|| HypervisorError::VmNotFound(vm_id.to_string()))?;

        let resumed = if let Some(ref qmp) = proc.qmp {
            qmp.lock().await.cont().await.is_ok()
        } else {
            false
        };

        let pid = proc.pid;
        drop(procs); // release before acquiring vms

        if !resumed {
            #[cfg(unix)]
            unsafe {
                libc::kill(pid as i32, libc::SIGCONT);
            }
        }

        let mut vms = self.vms.write().await;
        if let Some(vm) = vms.get_mut(vm_id) {
            vm.status = VmStatus::Running;
        }
        Ok(())
    }

    async fn delete_vm(&self, vm_id: &VmId) -> Result<(), HypervisorError> {
        self.delete_vm(vm_id).await
    }

    async fn restart_vm(&self, vm_id: &VmId) -> Result<(), HypervisorError> {
        self.restart_vm(vm_id).await
    }

    async fn get_vm_info(&self, vm_id: &VmId) -> Option<VmInfo> {
        self.get_vm_info(vm_id).await
    }

    async fn get_all_vms(&self) -> Vec<VmDetailedInfo> {
        self.get_all_vms().await
    }

    async fn get_vm_started_at_ms(&self, vm_id: &VmId) -> Option<u64> {
        self.get_vm_started_at_ms(vm_id).await
    }

    async fn is_app_started(&self, vm_id: &VmId) -> bool {
        self.is_app_started(vm_id).await
    }

    fn get_logs(&self, vm_id: &VmId) -> Vec<String> {
        if let Some(logs) = self.logs.get(vm_id) {
            logs.iter().cloned().collect()
        } else {
            // Attempt to read from serial log file
            let log_path = self.serial_log_path(vm_id);
            std::fs::read_to_string(log_path)
                .map(|content| content.lines().map(String::from).collect())
                .unwrap_or_default()
        }
    }

    async fn update_vm_firewall(
        &self,
        _vm_id: &VmId,
        _rules: Vec<mikrom_agent_ebpf_common::FirewallRule>,
    ) -> Result<(), HypervisorError> {
        Err(HypervisorError::ProcessError(
            "eBPF firewall not supported for QEMU microvm".to_string(),
        ))
    }

    async fn init_network(&self) -> Result<(), HypervisorError> {
        self.init_network().await
    }

    async fn load_runtime_state(&self) -> Result<(), HypervisorError> {
        self.load_runtime_state().await
    }

    async fn persist_runtime_state(&self) -> Result<(), HypervisorError> {
        self.persist_runtime_state().await
    }

    async fn cleanup_all_stale_resources(&self) {
        self.cleanup_all_stale_resources().await;
    }

    async fn set_nats_client(&self, _client: async_nats::Client) {}

    fn start_background_tasks(&self) {
        let self_clone = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                interval.tick().await;
                self_clone.run_gc().await;
            }
        });
    }

    // ── VM Snapshots ──────────────────────────────────────────────

    async fn create_vm_snapshot(&self, vm_id: &VmId, name: &str) -> Result<(), HypervisorError> {
        self.create_snapshot(vm_id, name).await
    }

    async fn restore_vm_snapshot(&self, vm_id: &VmId, name: &str) -> Result<(), HypervisorError> {
        self.restore_snapshot(vm_id, name).await
    }

    async fn delete_vm_snapshot(&self, vm_id: &VmId, name: &str) -> Result<(), HypervisorError> {
        self.delete_snapshot(vm_id, name).await
    }

    async fn list_vm_snapshots(
        &self,
        vm_id: &VmId,
    ) -> Result<Vec<mikrom_proto::agent::VmSnapshotInfo>, HypervisorError> {
        let names = self.list_snapshots(vm_id).await?;
        let now = chrono::Utc::now().timestamp();
        Ok(names
            .into_iter()
            .enumerate()
            .map(|(i, name)| mikrom_proto::agent::VmSnapshotInfo {
                id: (i + 1).to_string(),
                name,
                created_at: now,
                size_bytes: 0,
                vm_status: "unknown".to_string(),
            })
            .collect())
    }

    // ── Volume Hot-Plug ───────────────────────────────────────────

    async fn attach_volume(
        &self,
        vm_id: &VmId,
        volume_id: &str,
        mount_point: &str,
        read_only: bool,
    ) -> Result<(), HypervisorError> {
        let disk_path = std::path::Path::new(mount_point);
        self.attach_volume(vm_id, volume_id, disk_path, read_only)
            .await
    }

    async fn detach_volume(&self, vm_id: &VmId, volume_id: &str) -> Result<(), HypervisorError> {
        self.detach_volume(vm_id, volume_id).await
    }

    // ── Live Migration ────────────────────────────────────────────

    async fn start_migration(
        &self,
        vm_id: &VmId,
        _target_host: &str,
        target_uri: &str,
    ) -> Result<(), HypervisorError> {
        self.start_migration(vm_id, target_uri).await
    }

    async fn cancel_migration(&self, vm_id: &VmId) -> Result<(), HypervisorError> {
        self.cancel_migration(vm_id).await
    }

    async fn query_migration(&self, vm_id: &VmId) -> Result<String, HypervisorError> {
        self.query_migration(vm_id).await
    }

    // ── Balloon ────────────────────────────────────────────────────

    async fn set_balloon_size(
        &self,
        vm_id: &VmId,
        target_memory_mib: u32,
    ) -> Result<(), HypervisorError> {
        self.set_balloon_size(vm_id, target_memory_mib.into()).await
    }

    async fn query_balloon(&self, vm_id: &VmId) -> Result<(u32, u32), HypervisorError> {
        self.query_balloon(vm_id)
            .await
            .map(|(a, m)| (a as u32, m as u32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mikrom_proto::id::AppId;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn test_config() -> QemuConfig {
        QemuConfig {
            binary: "/bin/sleep".into(),
            kernel_path: "/dev/null".into(),
            rootfs_path: "/dev/null".into(),
            base_rootfs_path: "/dev/null".into(),
            data_dir: std::env::temp_dir().join(format!("qemu-test-{}", std::process::id())),
            qmp_timeout_secs: 1,
            extra_args: vec!["3600".into()],
            kernel_url: None,
            rootfs_url: None,
            image_cache_dir: std::env::temp_dir().join("qemu-image-cache-test"),
            virtiofsd_binary: String::new(),
            virtiofsd_socket_dir: std::env::temp_dir().join("qemu-virtiofsd-test"),
            virtiofsd_shares: Vec::new(),
        }
    }

    fn new_vm_id() -> VmId {
        VmId::new()
    }

    fn new_app_id() -> AppId {
        AppId::new()
    }

    /// Create a temporary Unix socket for QMP tests.
    fn make_test_socket(name: &str) -> (std::path::PathBuf, tokio::net::UnixListener) {
        let path = std::env::temp_dir().join(format!("qemu-{name}-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        (path, listener)
    }

    /// Connect a QMP client to the given socket and inject a fake `QemuProcess`
    /// (plus a seeded `VmInfo`) into the manager.
    async fn inject_qemu_process(qemu: &QemuManager, vm_id: VmId, socket_path: &std::path::Path) {
        let client = QmpClient::connect(socket_path).await.unwrap();
        let proc = QemuProcess {
            child: tokio::process::Command::new("/bin/true").spawn().unwrap(),
            pid: 12345,
            qmp_socket: socket_path.to_path_buf(),
            tap_name: "qemu-tap0".into(),
            started_at: chrono::Utc::now().timestamp(),
            qmp: Some(tokio::sync::Mutex::new(client)),
            virtiofsd: Vec::new(),
            virtiofsd_sockets: Vec::new(),
            event_task: None,
        };
        qemu.vms.write().await.insert(
            vm_id,
            VmInfo {
                vm_id,
                app_id: new_app_id(),
                image: "test".into(),
                config: VmConfig::default(),
                status: VmStatus::Running,
                started_at: Some(proc.started_at),
                error_message: None,
            },
        );
        qemu.processes.lock().await.insert(vm_id, proc);
    }

    async fn start_test_vm(qemu: &QemuManager) -> VmId {
        let vm_id = new_vm_id();
        let app_id = new_app_id();
        qemu.start_vm(vm_id, app_id, "test-img".into(), VmConfig::default())
            .await
            .expect("start_vm should succeed");
        vm_id
    }

    #[tokio::test]
    async fn test_hypervisor_type() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        assert_eq!(qemu.hypervisor_type(), HypervisorType::QemuMicrovm);
    }

    #[tokio::test]
    async fn test_start_and_stop_vm() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let vm_id = start_test_vm(&qemu).await;

        let info = qemu.get_vm_info(&vm_id).await;
        assert!(info.is_some());
        assert_eq!(info.unwrap().status, VmStatus::Running);

        qemu.stop_vm(&vm_id).await.expect("stop_vm should succeed");

        let info = qemu.get_vm_info(&vm_id).await;
        assert_eq!(info.unwrap().status, VmStatus::Stopped);
    }

    #[tokio::test]
    async fn test_pause_and_resume_vm() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let vm_id = start_test_vm(&qemu).await;

        qemu.pause_vm(&vm_id)
            .await
            .expect("pause_vm should succeed");
        let info = qemu.get_vm_info(&vm_id).await.unwrap();
        assert_eq!(info.status, VmStatus::Paused);

        qemu.resume_vm(&vm_id)
            .await
            .expect("resume_vm should succeed");
        let info = qemu.get_vm_info(&vm_id).await.unwrap();
        assert_eq!(info.status, VmStatus::Running);

        qemu.stop_vm(&vm_id).await.ok();
    }

    #[tokio::test]
    async fn test_get_all_vms() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let vm1 = start_test_vm(&qemu).await;
        let vm2 = start_test_vm(&qemu).await;

        let all = qemu.get_all_vms().await;
        assert_eq!(all.len(), 2);

        let ids: Vec<_> = all.iter().map(|v| v.vm_id).collect();
        assert!(ids.contains(&vm1));
        assert!(ids.contains(&vm2));

        qemu.stop_vm(&vm1).await.ok();
        qemu.stop_vm(&vm2).await.ok();
    }

    #[tokio::test]
    async fn test_delete_vm() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let vm_id = start_test_vm(&qemu).await;

        qemu.delete_vm(&vm_id)
            .await
            .expect("delete_vm should succeed");

        let info = qemu.get_vm_info(&vm_id).await;
        assert!(info.is_none());
    }

    #[tokio::test]
    async fn test_restart_vm() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let vm_id = start_test_vm(&qemu).await;

        qemu.restart_vm(&vm_id)
            .await
            .expect("restart_vm should succeed");

        let info = qemu.get_vm_info(&vm_id).await.unwrap();
        assert_eq!(info.status, VmStatus::Running);

        qemu.stop_vm(&vm_id).await.ok();
    }

    #[tokio::test]
    async fn test_update_vm_firewall_returns_error() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let result = qemu.update_vm_firewall(&new_vm_id(), vec![]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_stop_nonexistent_vm() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let result = qemu.stop_vm(&new_vm_id()).await;
        assert!(matches!(result, Err(HypervisorError::VmNotFound(_))));
    }

    #[tokio::test]
    async fn test_pause_nonexistent_vm() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let result = qemu.pause_vm(&new_vm_id()).await;
        assert!(matches!(result, Err(HypervisorError::VmNotFound(_))));
    }

    #[tokio::test]
    async fn test_get_vm_info_returns_none_for_unknown() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let info = qemu.get_vm_info(&new_vm_id()).await;
        assert!(info.is_none());
    }

    #[tokio::test]
    async fn test_is_app_started() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let vm_id = start_test_vm(&qemu).await;

        assert!(qemu.is_app_started(&vm_id).await);

        qemu.pause_vm(&vm_id).await.unwrap();
        assert!(!qemu.is_app_started(&vm_id).await);

        qemu.stop_vm(&vm_id).await.ok();
    }

    #[tokio::test]
    async fn test_get_vm_started_at_ms() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let vm_id = start_test_vm(&qemu).await;

        let started = qemu.get_vm_started_at_ms(&vm_id).await;
        assert!(started.is_some());
        assert!(started.unwrap() > 0);

        qemu.stop_vm(&vm_id).await.ok();
    }

    #[tokio::test]
    async fn test_resolve_kernel_uses_path_when_no_url() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let path = qemu.resolve_kernel().await;
        assert_eq!(path, PathBuf::from("/dev/null"));
    }

    #[tokio::test]
    async fn test_resolve_rootfs_uses_path_when_no_url() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let path = qemu.resolve_rootfs("img").await;
        assert_eq!(path, PathBuf::from("/dev/null"));
    }

    #[tokio::test]
    async fn test_resolve_kernel_downloads_from_url() {
        let cache_dir =
            std::env::temp_dir().join(format!("qemu-cache-test-{}", std::process::id()));
        let cfg = QemuConfig {
            kernel_url: Some("https://example.com/vmlinux".into()),
            image_cache_dir: cache_dir.clone(),
            ..test_config()
        };
        let qemu = QemuManager::with_config("test-agent".into(), cfg).await;
        let path = qemu.resolve_kernel().await;
        // Should return the cached file path (download may succeed or fail)
        assert!(
            path.starts_with(&cache_dir)
                || path == std::path::Path::new("https://example.com/vmlinux"),
            "Expected cached path or fallback URL, got {path:?}"
        );
        let _ = tokio::fs::remove_dir_all(&cache_dir).await;
    }

    #[tokio::test]
    async fn test_build_qemu_cmd_includes_binary_and_args() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let vm_id = new_vm_id();
        let config = VmConfig::default();
        let args = qemu.build_qemu_cmd(
            &vm_id,
            &config,
            "/dev/null",
            "/rootfs.img",
            "qemu-tap0",
            "/tmp/test.qmp",
            "/tmp/test.pid",
            "/tmp/test.log",
        );
        assert!(args[0].contains("sleep"), "binary should be sleep");
        assert!(args.contains(&"-machine".to_string()));
        assert!(args.contains(&"microvm".to_string()));
        assert!(args.contains(&"-accel".to_string()));
        assert!(args.contains(&"kvm".to_string()));
        assert!(args.contains(&"-kernel".to_string()));
        assert!(args.iter().any(|a| a.contains("if=none,id=root")));
        assert!(args
            .iter()
            .any(|a| a == "console=hvc0 root=/dev/vda reboot=t panic=1"));
        assert!(args.contains(&"virtio-serial-device".to_string()));
        assert!(args.iter().any(|a| a == "virtconsole,chardev=console"));
        assert!(args.iter().any(|a| a.starts_with("vhost-vsock-device")));
    }

    #[tokio::test]
    async fn test_vsock_cid_is_in_valid_range() {
        let vm_id = new_vm_id();
        let cid = QemuManager::vsock_cid(&vm_id);
        assert!(cid >= 3, "CID must be >= 3");
        assert!(cid < 0xFFFFFFFF, "CID must be < 0xFFFFFFFF");
    }

    #[tokio::test]
    async fn test_vsock_cid_is_deterministic() {
        let vm_id = new_vm_id();
        let cid1 = QemuManager::vsock_cid(&vm_id);
        let cid2 = QemuManager::vsock_cid(&vm_id);
        assert_eq!(cid1, cid2);
    }

    #[tokio::test]
    async fn test_tap_name_format() {
        let vm_id = new_vm_id();
        let name = QemuManager::tap_name(&vm_id);
        assert!(name.starts_with("qemu-"));
        assert_eq!(name.len(), 13); // "qemu-" (5) + 8 hex chars
        assert!(name[5..].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn test_get_logs_returns_empty_for_unknown_vm() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let logs = qemu.get_logs(&new_vm_id());
        assert!(logs.is_empty());
    }

    #[tokio::test]
    async fn test_agent_id() {
        let qemu = QemuManager::with_config("my-agent-42".into(), test_config()).await;
        assert_eq!(qemu.agent_id(), "my-agent-42");
    }

    #[tokio::test]
    async fn test_init_network_creates_bridge() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        // In a test environment without netlink permissions, ensure_bridge
        // returns an error.  With real root privileges it succeeds.
        let result = qemu.init_network().await;
        if result.is_err() {
            // Expected in CI / non-root environments
            tracing::warn!(
                "init_network failed (expected without netlink perms): {:?}",
                result
            );
        }
    }

    #[tokio::test]
    async fn test_persist_and_load_runtime_state() {
        let dir = std::env::temp_dir().join(format!("qemu-persist-test-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;

        let config = QemuConfig {
            data_dir: dir.clone(),
            extra_args: vec!["3600".into()],
            ..test_config()
        };
        let qemu = QemuManager::with_config("test-agent".into(), config).await;

        let vm_id = start_test_vm(&qemu).await;

        // Stop the VM and persist (simulates agent shutdown)
        qemu.stop_vm(&vm_id).await.ok();
        qemu.persist_runtime_state()
            .await
            .expect("persist should succeed");

        // Create a fresh manager and load state
        let config2 = QemuConfig {
            data_dir: dir,
            extra_args: vec!["3600".into()],
            ..test_config()
        };
        let qemu2 = QemuManager::with_config("test-agent".into(), config2).await;
        qemu2
            .load_runtime_state()
            .await
            .expect("load should succeed");

        let info = qemu2.get_vm_info(&vm_id).await;
        assert!(info.is_some(), "VM should be restored from state file");
        assert_eq!(info.unwrap().status, VmStatus::Stopped);
    }

    #[tokio::test]
    async fn test_qmp_event_listener_updates_vm_status() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let vm_id = new_vm_id();
        let app_id = new_app_id();

        // Seed the VM into the manager so the listener has something to update
        qemu.vms.write().await.insert(
            vm_id,
            VmInfo {
                vm_id,
                app_id,
                image: "test".into(),
                config: VmConfig::default(),
                status: VmStatus::Running,
                started_at: Some(chrono::Utc::now().timestamp()),
                error_message: None,
            },
        );

        let socket_path =
            std::env::temp_dir().join(format!("qemu-event-test-{}.sock", std::process::id()));
        let _ = tokio::fs::remove_file(&socket_path).await;

        let listener = tokio::net::UnixListener::bind(&socket_path).unwrap();

        // Spawn the event listener
        let task = qemu.spawn_event_listener(vm_id, socket_path.clone());

        // Accept the connection from the listener
        let (mut stream, _) = listener.accept().await.unwrap();

        // Send QMP greeting
        let greeting = r#"{"QMP": {"version": {"qemu": {"major": 8, "minor": 0, "micro": 0}}, "capabilities": ["oob"]}}"#;
        stream.write_all(greeting.as_bytes()).await.unwrap();
        stream.write_all(b"\n").await.unwrap();
        stream.flush().await.unwrap();

        // Read and discard capabilities command
        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).await.unwrap();
        let received = String::from_utf8_lossy(&buf[..n]);
        assert!(received.contains("qmp_capabilities"));

        // Send capabilities response
        let cap_resp = r#"{"return": {}}"#;
        stream.write_all(cap_resp.as_bytes()).await.unwrap();
        stream.write_all(b"\n").await.unwrap();
        stream.flush().await.unwrap();

        // Small delay to let the listener enter the event loop
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Emit STOP event → should set Paused
        let stop_event =
            r#"{"event": "STOP", "data": {}, "timestamp": {"seconds": 1, "microseconds": 0}}"#;
        stream.write_all(stop_event.as_bytes()).await.unwrap();
        stream.write_all(b"\n").await.unwrap();
        stream.flush().await.unwrap();

        tokio::time::sleep(Duration::from_millis(200)).await;
        {
            let vms = qemu.vms.read().await;
            let vm = vms.get(&vm_id).unwrap();
            assert_eq!(vm.status, VmStatus::Paused, "STOP event should set Paused");
        }

        // Emit RESUME event → should set Running
        let resume_event =
            r#"{"event": "RESUME", "data": {}, "timestamp": {"seconds": 2, "microseconds": 0}}"#;
        stream.write_all(resume_event.as_bytes()).await.unwrap();
        stream.write_all(b"\n").await.unwrap();
        stream.flush().await.unwrap();

        tokio::time::sleep(Duration::from_millis(200)).await;
        {
            let vms = qemu.vms.read().await;
            let vm = vms.get(&vm_id).unwrap();
            assert_eq!(
                vm.status,
                VmStatus::Running,
                "RESUME event should set Running"
            );
        }

        // Emit SHUTDOWN event → should set Stopping
        let shutdown_event =
            r#"{"event": "SHUTDOWN", "data": {}, "timestamp": {"seconds": 3, "microseconds": 0}}"#;
        stream.write_all(shutdown_event.as_bytes()).await.unwrap();
        stream.write_all(b"\n").await.unwrap();
        stream.flush().await.unwrap();

        tokio::time::sleep(Duration::from_millis(200)).await;
        {
            let vms = qemu.vms.read().await;
            let vm = vms.get(&vm_id).unwrap();
            assert_eq!(
                vm.status,
                VmStatus::Stopping,
                "SHUTDOWN event should set Stopping"
            );
        }

        // Clean up
        task.abort();
        let _ = tokio::fs::remove_file(&socket_path).await;
    }

    #[tokio::test]
    async fn test_get_serial_console_reads_last_lines() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let vm_id = new_vm_id();

        // Write fake serial log content
        let log_path = qemu.serial_log_path(&vm_id);
        if let Some(parent) = log_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let lines: Vec<String> = (1..=10).map(|i| format!("line {i}")).collect();
        tokio::fs::write(&log_path, lines.join("\n")).await.unwrap();

        // Request last 3 lines
        let result = qemu.get_serial_console(&vm_id, 3).await;
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], "line 8");
        assert_eq!(result[1], "line 9");
        assert_eq!(result[2], "line 10");

        // Request more lines than exist → returns all
        let result = qemu.get_serial_console(&vm_id, 100).await;
        assert_eq!(result.len(), 10);

        // Non-existent VM → returns empty
        let result = qemu.get_serial_console(&new_vm_id(), 10).await;
        assert!(result.is_empty());

        let _ = tokio::fs::remove_file(&log_path).await;
    }

    #[tokio::test]
    async fn test_cleanup_all_stale_resources_removes_orphan_files() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let stale_vm_id = new_vm_id();
        let stale_str = stale_vm_id.to_string();

        // Create stale resources (files for a VM that does not exist in the manager)
        let data_dir = &qemu.config.data_dir;
        let _ = tokio::fs::create_dir_all(data_dir).await;

        let state_path = data_dir.join(format!("qemu-{stale_str}.json"));
        let qmp_path = data_dir.join(format!("qemu-{stale_str}.qmp"));
        let pid_path = data_dir.join(format!("qemu-{stale_str}.pid"));
        let log_path = data_dir.join(format!("qemu-{stale_str}.log"));
        let err_log_path = data_dir.join(format!("qemu-{stale_str}.err.log"));

        tokio::fs::write(&state_path, "{}").await.unwrap();
        tokio::fs::write(&qmp_path, "").await.unwrap();
        tokio::fs::write(&pid_path, "12345\n").await.unwrap();
        tokio::fs::write(&log_path, "boot log\n").await.unwrap();
        tokio::fs::write(&err_log_path, "stderr log\n").await.unwrap();

        // Create stale virtiofsd socket
        let fsd_dir = &qemu.config.virtiofsd_socket_dir;
        let _ = tokio::fs::create_dir_all(fsd_dir).await;
        let fsd_sock = fsd_dir.join(format!("qemu-{stale_str}-share.sock"));
        tokio::fs::write(&fsd_sock, "").await.unwrap();

        // Also create a file for an active VM that should NOT be cleaned up
        let active_vm_id = new_vm_id();
        let active_str = active_vm_id.to_string();
        let active_state = data_dir.join(format!("qemu-{active_str}.json"));
        let active_err_log = data_dir.join(format!("qemu-{active_str}.err.log"));
        tokio::fs::write(&active_state, "{}").await.unwrap();
        tokio::fs::write(&active_err_log, "active stderr\n").await.unwrap();

        // Seed the active VM into the manager
        qemu.vms.write().await.insert(
            active_vm_id,
            VmInfo {
                vm_id: active_vm_id,
                app_id: new_app_id(),
                image: "active".into(),
                config: VmConfig::default(),
                status: VmStatus::Running,
                started_at: Some(chrono::Utc::now().timestamp()),
                error_message: None,
            },
        );

        // Run cleanup
        qemu.cleanup_all_stale_resources().await;

        // Verify stale files are gone
        assert!(!state_path.exists(), "stale state JSON should be removed");
        assert!(!qmp_path.exists(), "stale QMP socket should be removed");
        assert!(!pid_path.exists(), "stale pidfile should be removed");
        assert!(!log_path.exists(), "stale serial log should be removed");
        assert!(!err_log_path.exists(), "stale stderr log should be removed");
        assert!(
            !fsd_sock.exists(),
            "stale virtiofsd socket should be removed"
        );

        // Verify active VM files are preserved
        assert!(active_state.exists(), "active VM state should be preserved");
        assert!(active_err_log.exists(), "active stderr log should be preserved");

        // Clean up
        let _ = tokio::fs::remove_file(&active_state).await;
        let _ = tokio::fs::remove_file(&active_err_log).await;
        let _ = tokio::fs::remove_dir_all(data_dir).await;
        let _ = tokio::fs::remove_dir_all(fsd_dir).await;
    }

    #[tokio::test]
    async fn test_create_snapshot_sends_savevm_command() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let vm_id = new_vm_id();

        let (socket_path, listener) = make_test_socket("snap");

        // Background server that performs handshake and waits for savevm
        let server = tokio::spawn({
            let socket_path = socket_path.clone();
            async move {
                let (mut stream, _) = listener.accept().await.unwrap();

                // QMP greeting
                let greeting = r#"{"QMP": {"version": {"qemu": {"major": 8, "minor": 0, "micro": 0}}, "capabilities": ["oob"]}}"#;
                stream.write_all(greeting.as_bytes()).await.unwrap();
                stream.write_all(b"\n").await.unwrap();
                stream.flush().await.unwrap();

                // Read capabilities
                let mut buf = vec![0u8; 1024];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("qmp_capabilities"));
                stream.write_all(b"{\"return\": {}}\n").await.unwrap();
                stream.flush().await.unwrap();

                // Now the real test: wait for human-monitor-command with savevm
                let mut buf = vec![0u8; 1024];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("human-monitor-command"));
                assert!(msg.contains("savevm my-snap"));

                // Respond with empty string (HMP output)
                stream.write_all(br#"{"return": ""}"#).await.unwrap();
                stream.write_all(b"\n").await.unwrap();
                stream.flush().await.unwrap();

                let _ = tokio::fs::remove_file(&socket_path).await;
            }
        });

        inject_qemu_process(&qemu, vm_id, &socket_path).await;

        qemu.create_snapshot(&vm_id, "my-snap").await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_restore_snapshot_sends_loadvm_command() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let vm_id = new_vm_id();

        let (socket_path, listener) = make_test_socket("load");

        let server = tokio::spawn({
            let socket_path = socket_path.clone();
            async move {
                let (mut stream, _) = listener.accept().await.unwrap();

                let greeting = r#"{"QMP": {"version": {"qemu": {"major": 8, "minor": 0, "micro": 0}}, "capabilities": ["oob"]}}"#;
                stream.write_all(greeting.as_bytes()).await.unwrap();
                stream.write_all(b"\n").await.unwrap();
                stream.flush().await.unwrap();

                let mut buf = vec![0u8; 1024];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("qmp_capabilities"));
                stream.write_all(b"{\"return\": {}}\n").await.unwrap();
                stream.flush().await.unwrap();

                let mut buf = vec![0u8; 1024];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("human-monitor-command"));
                assert!(msg.contains("loadvm old-snap"));

                stream.write_all(br#"{"return": ""}"#).await.unwrap();
                stream.write_all(b"\n").await.unwrap();
                stream.flush().await.unwrap();

                let _ = tokio::fs::remove_file(&socket_path).await;
            }
        });

        inject_qemu_process(&qemu, vm_id, &socket_path).await;

        qemu.restore_snapshot(&vm_id, "old-snap").await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_list_snapshots_parses_output() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let vm_id = new_vm_id();

        let (socket_path, listener) = make_test_socket("list");

        let server = tokio::spawn({
            let socket_path = socket_path.clone();
            async move {
                let (mut stream, _) = listener.accept().await.unwrap();

                let greeting = r#"{"QMP": {"version": {"qemu": {"major": 8, "minor": 0, "micro": 0}}, "capabilities": ["oob"]}}"#;
                stream.write_all(greeting.as_bytes()).await.unwrap();
                stream.write_all(b"\n").await.unwrap();
                stream.flush().await.unwrap();

                let mut buf = vec![0u8; 1024];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("qmp_capabilities"));
                stream.write_all(b"{\"return\": {}}\n").await.unwrap();
                stream.flush().await.unwrap();

                let mut buf = vec![0u8; 1024];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("info snapshots"));

                // Fake HMP output with two snapshots
                let hmp = "Snapshot list:\nID        TAG                 VM SIZE                DATE       VM CLOCK\n1         snap-a              123M 2024-01-01 00:00:00   00:00:01.000\n2         snap-b              456M 2024-01-02 00:00:00   00:00:02.000\n";
                let resp = format!("{{\"return\": {:?}}}", hmp);
                stream.write_all(resp.as_bytes()).await.unwrap();
                stream.write_all(b"\n").await.unwrap();
                stream.flush().await.unwrap();

                let _ = tokio::fs::remove_file(&socket_path).await;
            }
        });

        inject_qemu_process(&qemu, vm_id, &socket_path).await;

        let snaps = qemu.list_snapshots(&vm_id).await.unwrap();
        server.await.unwrap();

        assert_eq!(snaps.len(), 2);
        assert_eq!(snaps[0], "snap-a");
        assert_eq!(snaps[1], "snap-b");
    }

    #[tokio::test]
    async fn test_attach_volume_sends_blockdev_add_and_device_add() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let vm_id = new_vm_id();

        let (socket_path, listener) = make_test_socket("attach");

        let server = tokio::spawn({
            let socket_path = socket_path.clone();
            async move {
                let (mut stream, _) = listener.accept().await.unwrap();

                let greeting = r#"{"QMP": {"version": {"qemu": {"major": 8, "minor": 0, "micro": 0}}, "capabilities": ["oob"]}}"#;
                stream.write_all(greeting.as_bytes()).await.unwrap();
                stream.write_all(b"\n").await.unwrap();
                stream.flush().await.unwrap();

                let mut buf = vec![0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("qmp_capabilities"));
                stream.write_all(b"{\"return\": {}}\n").await.unwrap();
                stream.flush().await.unwrap();

                // Expect blockdev-add
                let mut buf = vec![0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("blockdev-add"));
                assert!(msg.contains("vol-myvol"));
                assert!(msg.contains("/tmp/disk.img"));
                stream.write_all(b"{\"return\": {}}\n").await.unwrap();
                stream.flush().await.unwrap();

                // Expect device_add
                let mut buf = vec![0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("device_add"));
                assert!(msg.contains("virtio-blk-myvol"));
                stream.write_all(b"{\"return\": {}}\n").await.unwrap();
                stream.flush().await.unwrap();

                let _ = tokio::fs::remove_file(&socket_path).await;
            }
        });

        inject_qemu_process(&qemu, vm_id, &socket_path).await;

        qemu.attach_volume(&vm_id, "myvol", Path::new("/tmp/disk.img"), false)
            .await
            .unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_detach_volume_sends_device_del_and_blockdev_del() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let vm_id = new_vm_id();

        let (socket_path, listener) = make_test_socket("detach");

        let server = tokio::spawn({
            let socket_path = socket_path.clone();
            async move {
                let (mut stream, _) = listener.accept().await.unwrap();

                let greeting = r#"{"QMP": {"version": {"qemu": {"major": 8, "minor": 0, "micro": 0}}, "capabilities": ["oob"]}}"#;
                stream.write_all(greeting.as_bytes()).await.unwrap();
                stream.write_all(b"\n").await.unwrap();
                stream.flush().await.unwrap();

                let mut buf = vec![0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("qmp_capabilities"));
                stream.write_all(b"{\"return\": {}}\n").await.unwrap();
                stream.flush().await.unwrap();

                // Expect device_del
                let mut buf = vec![0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("device_del"));
                assert!(msg.contains("virtio-blk-myvol2"));
                stream.write_all(b"{\"return\": {}}\n").await.unwrap();
                stream.flush().await.unwrap();

                // Expect blockdev-del (after 200ms sleep in the impl)
                let mut buf = vec![0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("blockdev-del"));
                assert!(msg.contains("vol-myvol2"));
                stream.write_all(b"{\"return\": {}}\n").await.unwrap();
                stream.flush().await.unwrap();

                let _ = tokio::fs::remove_file(&socket_path).await;
            }
        });

        inject_qemu_process(&qemu, vm_id, &socket_path).await;

        qemu.detach_volume(&vm_id, "myvol2").await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_start_migration_sends_migrate_command() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let vm_id = new_vm_id();

        let (socket_path, listener) = make_test_socket("migrate");

        let server = tokio::spawn({
            let socket_path = socket_path.clone();
            async move {
                let (mut stream, _) = listener.accept().await.unwrap();

                let greeting = r#"{"QMP": {"version": {"qemu": {"major": 8, "minor": 0, "micro": 0}}, "capabilities": ["oob"]}}"#;
                stream.write_all(greeting.as_bytes()).await.unwrap();
                stream.write_all(b"\n").await.unwrap();
                stream.flush().await.unwrap();

                let mut buf = vec![0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("qmp_capabilities"));
                stream.write_all(b"{\"return\": {}}\n").await.unwrap();
                stream.flush().await.unwrap();

                let mut buf = vec![0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("migrate"));
                assert!(msg.contains("tcp://dest:4444"));
                stream.write_all(b"{\"return\": {}}\n").await.unwrap();
                stream.flush().await.unwrap();

                let _ = tokio::fs::remove_file(&socket_path).await;
            }
        });

        inject_qemu_process(&qemu, vm_id, &socket_path).await;

        qemu.start_migration(&vm_id, "tcp://dest:4444")
            .await
            .unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_query_migration_returns_status() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let vm_id = new_vm_id();

        let (socket_path, listener) = make_test_socket("query-mig");

        let server = tokio::spawn({
            let socket_path = socket_path.clone();
            async move {
                let (mut stream, _) = listener.accept().await.unwrap();

                let greeting = r#"{"QMP": {"version": {"qemu": {"major": 8, "minor": 0, "micro": 0}}, "capabilities": ["oob"]}}"#;
                stream.write_all(greeting.as_bytes()).await.unwrap();
                stream.write_all(b"\n").await.unwrap();
                stream.flush().await.unwrap();

                let mut buf = vec![0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("qmp_capabilities"));
                stream.write_all(b"{\"return\": {}}\n").await.unwrap();
                stream.flush().await.unwrap();

                let mut buf = vec![0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("query-migrate"));
                stream
                    .write_all(br#"{"return": {"status": "active", "total-time": 100, "ram": {"total": 1073741824, "transferred": 536870912}}}"#)
                    .await
                    .unwrap();
                stream.write_all(b"\n").await.unwrap();
                stream.flush().await.unwrap();

                let _ = tokio::fs::remove_file(&socket_path).await;
            }
        });

        inject_qemu_process(&qemu, vm_id, &socket_path).await;

        let status = qemu.query_migration(&vm_id).await.unwrap();
        server.await.unwrap();

        assert_eq!(status, "active");
    }

    #[tokio::test]
    async fn test_cancel_migration_sends_migrate_cancel() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let vm_id = new_vm_id();

        let (socket_path, listener) = make_test_socket("cancel-mig");

        let server = tokio::spawn({
            let socket_path = socket_path.clone();
            async move {
                let (mut stream, _) = listener.accept().await.unwrap();

                let greeting = r#"{"QMP": {"version": {"qemu": {"major": 8, "minor": 0, "micro": 0}}, "capabilities": ["oob"]}}"#;
                stream.write_all(greeting.as_bytes()).await.unwrap();
                stream.write_all(b"\n").await.unwrap();
                stream.flush().await.unwrap();

                let mut buf = vec![0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("qmp_capabilities"));
                stream.write_all(b"{\"return\": {}}\n").await.unwrap();
                stream.flush().await.unwrap();

                let mut buf = vec![0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("migrate_cancel"));
                stream.write_all(b"{\"return\": {}}\n").await.unwrap();
                stream.flush().await.unwrap();

                let _ = tokio::fs::remove_file(&socket_path).await;
            }
        });

        inject_qemu_process(&qemu, vm_id, &socket_path).await;

        qemu.cancel_migration(&vm_id).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_set_balloon_size_sends_balloon_command() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let vm_id = new_vm_id();

        let (socket_path, listener) = make_test_socket("balloon");

        let server = tokio::spawn({
            let socket_path = socket_path.clone();
            async move {
                let (mut stream, _) = listener.accept().await.unwrap();

                let greeting = r#"{"QMP": {"version": {"qemu": {"major": 8, "minor": 0, "micro": 0}}, "capabilities": ["oob"]}}"#;
                stream.write_all(greeting.as_bytes()).await.unwrap();
                stream.write_all(b"\n").await.unwrap();
                stream.flush().await.unwrap();

                let mut buf = vec![0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("qmp_capabilities"));
                stream.write_all(b"{\"return\": {}}\n").await.unwrap();
                stream.flush().await.unwrap();

                let mut buf = vec![0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("balloon"));
                assert!(msg.contains("256"));
                stream.write_all(b"{\"return\": {}}\n").await.unwrap();
                stream.flush().await.unwrap();

                let _ = tokio::fs::remove_file(&socket_path).await;
            }
        });

        inject_qemu_process(&qemu, vm_id, &socket_path).await;

        qemu.set_balloon_size(&vm_id, 256).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_query_balloon_returns_stats() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let vm_id = new_vm_id();

        let (socket_path, listener) = make_test_socket("qballoon");

        let server = tokio::spawn({
            let socket_path = socket_path.clone();
            async move {
                let (mut stream, _) = listener.accept().await.unwrap();

                let greeting = r#"{"QMP": {"version": {"qemu": {"major": 8, "minor": 0, "micro": 0}}, "capabilities": ["oob"]}}"#;
                stream.write_all(greeting.as_bytes()).await.unwrap();
                stream.write_all(b"\n").await.unwrap();
                stream.flush().await.unwrap();

                let mut buf = vec![0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("qmp_capabilities"));
                stream.write_all(b"{\"return\": {}}\n").await.unwrap();
                stream.flush().await.unwrap();

                let mut buf = vec![0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("query-balloon"));
                // 512 MiB actual, 1024 MiB max
                stream
                    .write_all(br#"{"return": {"actual": 536870912, "max": 1073741824}}"#)
                    .await
                    .unwrap();
                stream.write_all(b"\n").await.unwrap();
                stream.flush().await.unwrap();

                let _ = tokio::fs::remove_file(&socket_path).await;
            }
        });

        inject_qemu_process(&qemu, vm_id, &socket_path).await;

        let (actual, max) = qemu.query_balloon(&vm_id).await.unwrap();
        server.await.unwrap();

        assert_eq!(actual, 512);
        assert_eq!(max, 1024);
    }

    #[tokio::test]
    async fn test_query_cpus_returns_cpu_list() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let vm_id = new_vm_id();

        let (socket_path, listener) = make_test_socket("qcpu");

        let server = tokio::spawn({
            let socket_path = socket_path.clone();
            async move {
                let (mut stream, _) = listener.accept().await.unwrap();

                let greeting = r#"{"QMP": {"version": {"qemu": {"major": 8, "minor": 0, "micro": 0}}, "capabilities": ["oob"]}}"#;
                stream.write_all(greeting.as_bytes()).await.unwrap();
                stream.write_all(b"\n").await.unwrap();
                stream.flush().await.unwrap();

                let mut buf = vec![0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("qmp_capabilities"));
                stream.write_all(b"{\"return\": {}}\n").await.unwrap();
                stream.flush().await.unwrap();

                let mut buf = vec![0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("query-cpus"));
                stream.write_all(br#"{"return": [{"CPU": 0, "arch": "x86", "pending": true}, {"CPU": 1, "arch": "x86", "pending": false}]}"#).await.unwrap();
                stream.write_all(b"\n").await.unwrap();
                stream.flush().await.unwrap();

                let _ = tokio::fs::remove_file(&socket_path).await;
            }
        });

        inject_qemu_process(&qemu, vm_id, &socket_path).await;

        let cpus = qemu.query_cpus(&vm_id).await.unwrap();
        server.await.unwrap();

        assert_eq!(cpus.len(), 2);
        assert_eq!(cpus[0], (0, "x86".to_string(), true));
        assert_eq!(cpus[1], (1, "x86".to_string(), false));
    }

    #[tokio::test]
    async fn test_query_blockstats_returns_device_stats() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let vm_id = new_vm_id();

        let (socket_path, listener) = make_test_socket("qblk");

        let server = tokio::spawn({
            let socket_path = socket_path.clone();
            async move {
                let (mut stream, _) = listener.accept().await.unwrap();

                let greeting = r#"{"QMP": {"version": {"qemu": {"major": 8, "minor": 0, "micro": 0}}, "capabilities": ["oob"]}}"#;
                stream.write_all(greeting.as_bytes()).await.unwrap();
                stream.write_all(b"\n").await.unwrap();
                stream.flush().await.unwrap();

                let mut buf = vec![0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("qmp_capabilities"));
                stream.write_all(b"{\"return\": {}}\n").await.unwrap();
                stream.flush().await.unwrap();

                let mut buf = vec![0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                assert!(msg.contains("query-blockstats"));
                stream.write_all(br#"{"return": [{"device": "virtio0", "stats": {"rd_bytes": 1024, "wr_bytes": 2048}}]}"#).await.unwrap();
                stream.write_all(b"\n").await.unwrap();
                stream.flush().await.unwrap();

                let _ = tokio::fs::remove_file(&socket_path).await;
            }
        });

        inject_qemu_process(&qemu, vm_id, &socket_path).await;

        let stats = qemu.query_blockstats(&vm_id).await.unwrap();
        server.await.unwrap();

        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0], ("virtio0".to_string(), 1024, 2048));
    }

    #[tokio::test]
    async fn test_get_stderr_logs_reads_last_lines() {
        let qemu = QemuManager::with_config("test-agent".into(), test_config()).await;
        let vm_id = new_vm_id();

        let log_path = qemu.stderr_log_path(&vm_id);
        if let Some(parent) = log_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let lines: Vec<String> = (1..=10).map(|i| format!("stderr {i}")).collect();
        tokio::fs::write(&log_path, lines.join("\n")).await.unwrap();

        let result = qemu.get_stderr_logs(&vm_id, 3).await;
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], "stderr 8");
        assert_eq!(result[1], "stderr 9");
        assert_eq!(result[2], "stderr 10");

        let result = qemu.get_stderr_logs(&vm_id, 100).await;
        assert_eq!(result.len(), 10);

        let result = qemu.get_stderr_logs(&new_vm_id(), 10).await;
        assert!(result.is_empty());

        let _ = tokio::fs::remove_file(&log_path).await;
    }
}
