use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, RwLock};

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
pub enum VmStatus {
    Starting,
    Running,
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
    pub env: std::collections::HashMap<String, String>,
    pub ip_address: Option<String>,
    pub gateway: Option<String>,
    pub mac_address: Option<String>,
    pub volumes: Vec<Volume>,
}

struct VmProcess {
    child: tokio::process::Child,
    socket_path: String,
    tap_name: Option<String>,
    log_task: tokio::task::JoinHandle<()>,
}

#[derive(Clone)]
pub struct FirecrackerManager {
    vms: Arc<RwLock<HashMap<String, VmInfo>>>,
    processes: Arc<Mutex<HashMap<String, VmProcess>>>,
    fc_config: FirecrackerConfig,
    logs: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

// ── HTTP-over-Unix-socket helper ──────────────────────────────────────────────

/// Send a PUT request to the Firecracker API socket and return Ok on 2xx.
async fn fc_put(socket_path: &str, api_path: &str, body: &str) -> Result<(), FirecrackerError> {
    let stream = tokio::net::UnixStream::connect(socket_path)
        .await
        .map_err(|e| FirecrackerError::ApiError {
            path: api_path.to_string(),
            msg: format!("connect: {e}"),
        })?;

    let (reader, mut writer) = tokio::io::split(stream);

    let request = format!(
        "PUT {api_path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    writer
        .write_all(request.as_bytes())
        .await
        .map_err(|e| FirecrackerError::ApiError {
            path: api_path.to_string(),
            msg: format!("write: {e}"),
        })?;

    // Flush writer half before reading the response.
    writer
        .flush()
        .await
        .map_err(|e| FirecrackerError::ApiError {
            path: api_path.to_string(),
            msg: format!("flush: {e}"),
        })?;

    // Read only the status line — Firecracker responds with 204 No Content.
    let mut buf_reader = BufReader::new(reader);
    let mut status_line = String::new();
    buf_reader
        .read_line(&mut status_line)
        .await
        .map_err(|e| FirecrackerError::ApiError {
            path: api_path.to_string(),
            msg: format!("read: {e}"),
        })?;

    if status_line.contains(" 2") {
        Ok(())
    } else {
        Err(FirecrackerError::ApiError {
            path: api_path.to_string(),
            msg: status_line.trim().to_string(),
        })
    }
}

/// Poll until the Unix socket file appears (Firecracker is ready to accept API calls).
async fn wait_for_socket(path: &str, timeout: Duration) -> Result<(), FirecrackerError> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if std::path::Path::new(path).exists() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(FirecrackerError::SocketTimeout(path.to_string()));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

// ── FirecrackerConfig ─────────────────────────────────────────────────────────

/// Runtime configuration for `FirecrackerManager`.  `new()` reads from
/// environment variables; `with_config()` accepts an explicit instance so
/// tests can inject values without touching process-global env vars.
#[derive(Clone, Debug)]
pub struct FirecrackerConfig {
    /// Path to an uncompressed Linux kernel (`vmlinux`).
    /// When `None` the manager runs in stub mode (no process is spawned).
    pub kernel_path: Option<String>,
    /// Path to the Firecracker binary (default: `"firecracker"`).
    pub binary: String,
    /// Default rootfs ext4 image.  Used when the `image` field of a
    /// `StartVmRequest` is not a valid path on the host filesystem.
    pub rootfs_path: String,
}

impl FirecrackerConfig {
    pub fn from_env() -> Self {
        Self {
            kernel_path: std::env::var("FC_KERNEL_PATH").ok(),
            binary: std::env::var("FC_BINARY").unwrap_or_else(|_| "firecracker".to_string()),
            rootfs_path: std::env::var("FC_ROOTFS_PATH")
                .unwrap_or_else(|_| "/opt/firecracker/rootfs.ext4".to_string()),
        }
    }

    /// Convenience: stub mode — no kernel path set, so no process is ever spawned.
    pub fn stub() -> Self {
        Self {
            kernel_path: None,
            binary: "firecracker".to_string(),
            rootfs_path: "/opt/firecracker/rootfs.ext4".to_string(),
        }
    }
}

// ── FirecrackerManager ────────────────────────────────────────────────────────

impl FirecrackerManager {
    /// Create a manager whose configuration is read from environment variables.
    pub fn new() -> Self {
        Self::with_config(FirecrackerConfig::from_env())
    }

    /// Create a manager with an explicit configuration (useful for tests).
    pub fn with_config(fc_config: FirecrackerConfig) -> Self {
        Self {
            vms: Arc::new(RwLock::new(HashMap::new())),
            processes: Arc::new(Mutex::new(HashMap::new())),
            fc_config,
            logs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn start_vm(
        &self,
        vm_id: String,
        app_id: String,
        image: String,
        config: VmConfig,
    ) -> Result<(), FirecrackerError> {
        // Reject duplicate vm_id.
        {
            let vms = self.vms.read().await;
            if vms.contains_key(&vm_id) {
                return Err(FirecrackerError::StartFailed(
                    "VM already exists".to_string(),
                ));
            }
        }

        let vm_info = VmInfo {
            vm_id: vm_id.clone(),
            app_id,
            image: image.clone(),
            config: config.clone(),
            status: VmStatus::Starting,
            started_at: None,
            error_message: None,
        };
        self.vms.write().await.insert(vm_id.clone(), vm_info);

        // Add initial log entry
        {
            let mut l = self.logs.write().await;
            l.entry(vm_id.clone())
                .or_default()
                .push(format!("[agent] Initializing VM {}...", vm_id));
        }

        // ── Real mode: kernel_path is configured ──────────────────────────────
        let kernel_path = match self.fc_config.kernel_path.clone() {
            Some(p) => p,
            None => {
                // Stub mode: state-machine only, no process spawned.
                return Ok(());
            }
        };

        let fc_binary = &self.fc_config.binary;
        let base_rootfs = if std::path::Path::new(&image).exists() {
            image.clone()
        } else {
            self.fc_config.rootfs_path.clone()
        };

        // Copy the base rootfs to a per-VM writable path so Firecracker can
        // open it with write permissions even when the source is mounted :ro.
        let rootfs_path = format!("/tmp/fc-{vm_id}-rootfs.ext4");
        if let Err(e) = tokio::fs::copy(&base_rootfs, &rootfs_path).await {
            self.set_failed(&vm_id, e.to_string()).await;
            return Err(FirecrackerError::StartFailed(format!(
                "failed to copy rootfs {base_rootfs} → {rootfs_path}: {e}"
            )));
        }

        let socket_path = format!("/tmp/fc-{vm_id}.sock");

        // Remove any stale socket from a previous run.
        let _ = tokio::fs::remove_file(&socket_path).await;

        // Spawn the Firecracker process.
        let mut child = match tokio::process::Command::new(fc_binary)
            .arg("--api-sock")
            .arg(&socket_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                self.set_failed(&vm_id, e.to_string()).await;
                return Err(FirecrackerError::ProcessError(e.to_string()));
            }
        };

        // Capture logs in a background task
        let stdout = child.stdout.take().expect("Failed to take stdout");
        let stderr = child.stderr.take().expect("Failed to take stderr");
        let vm_id_clone = vm_id.clone();
        let logs_clone = self.logs.clone();

        let log_task = tokio::spawn(async move {
            let mut stdout_lines = BufReader::new(stdout).lines();
            let mut stderr_lines = BufReader::new(stderr).lines();

            loop {
                let line = tokio::select! {
                    Ok(Some(line)) = stdout_lines.next_line() => Some(line),
                    Ok(Some(line)) = stderr_lines.next_line() => Some(format!("[stderr] {}", line)),
                    else => None,
                };

                if let Some(l) = line {
                    let mut logs = logs_clone.write().await;
                    let vm_logs = logs
                        .entry(vm_id_clone.clone())
                        .or_insert_with(|| Vec::with_capacity(1000));
                    if vm_logs.len() >= 1000 {
                        vm_logs.remove(0);
                    }
                    vm_logs.push(l);
                } else {
                    break;
                }
            }
        });

        // Wait for the API socket to appear (up to 5 s).
        if let Err(e) = wait_for_socket(&socket_path, Duration::from_secs(5)).await {
            let _ = child.kill().await;
            self.set_failed(&vm_id, e.to_string()).await;
            return Err(e);
        }

        // ── Networking setup ───────────────────────────────────────────────────
        let tap_name = if config.ip_address.is_some() {
            Some(self.setup_tap(&vm_id).await?)
        } else {
            None
        };

        // Configure machine resources.
        let machine_config = serde_json::json!({
            "vcpu_count": config.vcpus,
            "mem_size_mib": config.memory_mib,
            "smt": false,
            "track_dirty_pages": false
        })
        .to_string();

        let mut boot_args = "console=ttyS0 reboot=k panic=1".to_string();
        if let (Some(ip), Some(gw)) = (&config.ip_address, &config.gateway) {
            boot_args.push_str(&format!(" ip={}::{}::eth0:off", ip, gw));
        }

        let boot_source = serde_json::json!({
            "kernel_image_path": kernel_path,
            "boot_args": boot_args
        })
        .to_string();

        let drives = serde_json::json!({
            "drive_id": "rootfs",
            "path_on_host": rootfs_path,
            "is_root_device": true,
            "is_read_only": false
        })
        .to_string();

        let network_interface = if let Some(tap) = &tap_name {
            Some(
                serde_json::json!({
                    "iface_id": "eth0",
                    "guest_mac": config.mac_address.as_deref().unwrap_or("AA:BB:CC:DD:EE:01"),
                    "host_dev_name": tap
                })
                .to_string(),
            )
        } else {
            None
        };

        let instance_start = serde_json::json!({
            "action_type": "InstanceStart"
        })
        .to_string();

        // ── API calls ──────────────────────────────────────────────────────────
        fc_put(&socket_path, "/machine-config", &machine_config).await?;
        fc_put(&socket_path, "/boot-source", &boot_source).await?;
        fc_put(&socket_path, "/drives/rootfs", &drives).await?;

        if let Some(net_json) = &network_interface {
            fc_put(&socket_path, "/network-interfaces/eth0", net_json).await?;
        }

        // ── Attach additional volumes ──────────────────────────────────────────
        for vol in &config.volumes {
            let vol_path = self.ensure_volume(&vol.volume_id, vol.size_mib).await?;
            let drive_json = serde_json::json!({
                "drive_id": vol.volume_id,
                "path_on_host": vol_path,
                "is_root_device": false,
                "is_read_only": vol.read_only
            })
            .to_string();
            fc_put(
                &socket_path,
                &format!("/drives/{}", vol.volume_id),
                &drive_json,
            )
            .await?;
        }

        fc_put(&socket_path, "/actions", &instance_start).await?;

        // VM is booting — mark as Running and store the process handle.
        {
            let mut vms = self.vms.write().await;
            if let Some(vm) = vms.get_mut(&vm_id) {
                vm.status = VmStatus::Running;
                vm.started_at = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0),
                );
            }
        }
        self.processes.lock().await.insert(
            vm_id,
            VmProcess {
                child,
                socket_path,
                tap_name,
                log_task,
            },
        );

        Ok(())
    }

    pub async fn stop_vm(&self, vm_id: &str) -> Result<(), FirecrackerError> {
        {
            let mut vms = self.vms.write().await;
            match vms.get_mut(vm_id) {
                Some(vm) => vm.status = VmStatus::Stopping,
                None => return Err(FirecrackerError::VmNotFound(vm_id.to_string())),
            }
        }

        // If there is a real process, kill it and clean up.
        if let Some(mut proc) = self.processes.lock().await.remove(vm_id) {
            proc.log_task.abort();
            let _ = proc.child.kill().await;
            let _ = proc.child.wait().await;
            let _ = tokio::fs::remove_file(&proc.socket_path).await;
            let _ = tokio::fs::remove_file(format!("/tmp/fc-{vm_id}-rootfs.ext4")).await;

            if let Some(tap) = &proc.tap_name {
                self.cleanup_tap(tap).await;
            }

            let mut vms = self.vms.write().await;
            if let Some(vm) = vms.get_mut(vm_id) {
                vm.status = VmStatus::Stopped;
            }
        }

        Ok(())
    }

    pub async fn get_vm_status(&self, vm_id: &str) -> Result<VmStatus, FirecrackerError> {
        let vms = self.vms.read().await;
        match vms.get(vm_id) {
            Some(vm) => Ok(vm.status),
            None => Err(FirecrackerError::VmNotFound(vm_id.to_string())),
        }
    }

    pub async fn list_vms(&self) -> Vec<VmInfo> {
        self.vms.read().await.values().cloned().collect()
    }

    pub async fn get_vm(&self, vm_id: &str) -> Option<VmInfo> {
        self.vms.read().await.get(vm_id).cloned()
    }

    pub async fn get_logs(&self, vm_id: &str) -> Vec<String> {
        self.logs
            .read()
            .await
            .get(vm_id)
            .cloned()
            .unwrap_or_default()
    }

    pub async fn get_pids(&self) -> HashMap<String, u32> {
        let mut pids = HashMap::new();
        let processes = self.processes.lock().await;
        for (vm_id, proc) in processes.iter() {
            if let Some(pid) = proc.child.id() {
                pids.insert(vm_id.clone(), pid);
            }
        }
        pids
    }

    async fn ensure_volume(
        &self,
        volume_id: &str,
        size_mib: u64,
    ) -> Result<String, FirecrackerError> {
        let vol_dir = "/tmp/mikrom-volumes"; // Using /tmp for now for dev permissions
        tokio::fs::create_dir_all(vol_dir).await.map_err(|e| {
            FirecrackerError::ProcessError(format!("Failed to create volumes dir: {}", e))
        })?;

        let vol_path = format!("{}/{}.ext4", vol_dir, volume_id);
        if !std::path::Path::new(&vol_path).exists() {
            // Create a sparse file
            let file = tokio::fs::File::create(&vol_path).await.map_err(|e| {
                FirecrackerError::ProcessError(format!("Failed to create volume file: {}", e))
            })?;
            file.set_len(size_mib * 1024 * 1024).await.map_err(|e| {
                FirecrackerError::ProcessError(format!("Failed to set volume size: {}", e))
            })?;
        }

        Ok(vol_path)
    }

    async fn set_failed(&self, vm_id: &str, msg: String) {
        let mut vms = self.vms.write().await;
        if let Some(vm) = vms.get_mut(vm_id) {
            vm.status = VmStatus::Failed;
            vm.error_message = Some(msg);
        }
    }

    async fn setup_tap(&self, vm_id: &str) -> Result<String, FirecrackerError> {
        let tap_name = format!("m-tap-{}", &vm_id[..8]);

        // Clean up if it exists
        let _ = tokio::process::Command::new("ip")
            .args(["link", "del", &tap_name])
            .output()
            .await;

        let output = tokio::process::Command::new("ip")
            .args(["tuntap", "add", "dev", &tap_name, "mode", "tap"])
            .output()
            .await
            .map_err(|e| FirecrackerError::ProcessError(format!("Failed to create TAP: {}", e)))?;

        if !output.status.success() {
            return Err(FirecrackerError::ProcessError(format!(
                "TAP creation failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let output = tokio::process::Command::new("ip")
            .args(["link", "set", &tap_name, "up"])
            .output()
            .await
            .map_err(|e| FirecrackerError::ProcessError(format!("Failed to set TAP up: {}", e)))?;

        if !output.status.success() {
            return Err(FirecrackerError::ProcessError(format!(
                "Failed to set TAP up: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(tap_name)
    }

    async fn cleanup_tap(&self, tap_name: &str) {
        let _ = tokio::process::Command::new("ip")
            .args(["link", "del", tap_name])
            .output()
            .await;
    }
}

impl Default for FirecrackerManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl FirecrackerManager {
    pub async fn set_status_for_test(&self, vm_id: &str, status: VmStatus) {
        if let Some(vm) = self.vms.write().await.get_mut(vm_id) {
            vm.status = status;
        }
    }

    /// Insert a real child process into the process map so `stop_vm` real-mode
    /// path can be exercised without going through the full `start_vm` flow.
    async fn insert_process_for_test(
        &self,
        vm_id: &str,
        child: tokio::process::Child,
        socket_path: String,
    ) {
        let log_task = tokio::spawn(async {});
        self.processes.lock().await.insert(
            vm_id.to_string(),
            VmProcess {
                child,
                socket_path,
                tap_name: None,
                log_task,
            },
        );
    }

    /// Returns true if there is a live process handle stored for this vm_id.
    async fn has_process(&self, vm_id: &str) -> bool {
        self.processes.lock().await.contains_key(vm_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> VmConfig {
        VmConfig {
            vcpus: 1,
            memory_mib: 256,
            disk_mib: 1024,
            env: Default::default(),
            ip_address: None,
            gateway: None,
            mac_address: None,
            volumes: vec![],
        }
    }

    async fn start(mgr: &FirecrackerManager, vm_id: &str) {
        mgr.start_vm(
            vm_id.to_string(),
            "app-1".to_string(),
            "nginx:latest".to_string(),
            config(),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_start_vm_succeeds() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        assert!(
            mgr.start_vm(
                "vm-1".to_string(),
                "app-1".to_string(),
                "img".to_string(),
                config()
            )
            .await
            .is_ok()
        );
    }

    #[tokio::test]
    async fn test_started_vm_has_starting_status() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        start(&mgr, "vm-1").await;
        assert_eq!(mgr.get_vm_status("vm-1").await.unwrap(), VmStatus::Starting);
    }

    #[tokio::test]
    async fn test_start_duplicate_vm_fails() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        start(&mgr, "vm-1").await;
        let result = mgr
            .start_vm(
                "vm-1".to_string(),
                "app-1".to_string(),
                "img".to_string(),
                config(),
            )
            .await;
        assert!(matches!(result, Err(FirecrackerError::StartFailed(_))));
    }

    #[tokio::test]
    async fn test_stop_vm_transitions_to_stopping() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        start(&mgr, "vm-1").await;
        assert!(mgr.stop_vm("vm-1").await.is_ok());
        // In stub mode (no FC_KERNEL_PATH) there is no process, so status stays Stopping.
        assert_eq!(mgr.get_vm_status("vm-1").await.unwrap(), VmStatus::Stopping);
    }

    #[tokio::test]
    async fn test_stop_nonexistent_vm_returns_error() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        assert!(matches!(
            mgr.stop_vm("ghost").await,
            Err(FirecrackerError::VmNotFound(_))
        ));
    }

    #[tokio::test]
    async fn test_get_status_nonexistent_returns_error() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        assert!(matches!(
            mgr.get_vm_status("ghost").await,
            Err(FirecrackerError::VmNotFound(_))
        ));
    }

    #[tokio::test]
    async fn test_list_vms_empty() {
        assert!(
            FirecrackerManager::with_config(FirecrackerConfig::stub())
                .list_vms()
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn test_list_vms_after_starts() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        start(&mgr, "vm-1").await;
        start(&mgr, "vm-2").await;
        assert_eq!(mgr.list_vms().await.len(), 2);
    }

    #[tokio::test]
    async fn test_get_vm_returns_correct_info() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        mgr.start_vm(
            "vm-1".to_string(),
            "app-42".to_string(),
            "ubuntu:24.04".to_string(),
            config(),
        )
        .await
        .unwrap();
        let vm = mgr.get_vm("vm-1").await.unwrap();
        assert_eq!(vm.app_id, "app-42");
        assert_eq!(vm.image, "ubuntu:24.04");
        assert_eq!(vm.config.vcpus, 1);
        assert_eq!(vm.config.memory_mib, 256);
        assert!(vm.config.volumes.is_empty());
    }

    #[tokio::test]
    async fn test_get_vm_nonexistent_returns_none() {
        assert!(
            FirecrackerManager::with_config(FirecrackerConfig::stub())
                .get_vm("ghost")
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_vm_status_default_is_stopped() {
        assert_eq!(VmStatus::default(), VmStatus::Stopped);
    }

    #[tokio::test]
    async fn test_vm_info_serialization_roundtrip() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        start(&mgr, "vm-1").await;
        let vm = mgr.get_vm("vm-1").await.unwrap();
        let json = serde_json::to_string(&vm).unwrap();
        let restored: VmInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.vm_id, "vm-1");
        assert_eq!(restored.status, VmStatus::Starting);
    }

    #[tokio::test]
    async fn test_vm_config_with_env_vars() {
        let mut env = std::collections::HashMap::new();
        env.insert("PORT".to_string(), "3000".to_string());
        env.insert("ENV".to_string(), "prod".to_string());
        let cfg = VmConfig {
            vcpus: 2,
            memory_mib: 512,
            disk_mib: 2048,
            env,
            ip_address: None,
            gateway: None,
            mac_address: None,
            volumes: vec![],
        };
        assert_eq!(cfg.env.get("PORT").unwrap(), "3000");
        assert_eq!(cfg.vcpus, 2);
    }

    #[tokio::test]
    async fn test_error_messages_contain_vm_id() {
        let err = FirecrackerError::VmNotFound("vm-99".to_string());
        assert!(err.to_string().contains("vm-99"));
        let err2 = FirecrackerError::StartFailed("already exists".to_string());
        assert!(err2.to_string().contains("already exists"));
        let err3 = FirecrackerError::StopFailed("busy".to_string());
        assert!(err3.to_string().contains("busy"));
    }

    #[tokio::test]
    async fn test_set_status_for_test_to_running() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        start(&mgr, "vm-1").await;
        mgr.set_status_for_test("vm-1", VmStatus::Running).await;
        assert_eq!(mgr.get_vm_status("vm-1").await.unwrap(), VmStatus::Running);
    }

    #[tokio::test]
    async fn test_set_status_for_test_to_stopped() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        start(&mgr, "vm-1").await;
        mgr.set_status_for_test("vm-1", VmStatus::Stopped).await;
        assert_eq!(mgr.get_vm_status("vm-1").await.unwrap(), VmStatus::Stopped);
    }

    #[tokio::test]
    async fn test_set_status_for_test_to_failed() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        start(&mgr, "vm-1").await;
        mgr.set_status_for_test("vm-1", VmStatus::Failed).await;
        assert_eq!(mgr.get_vm_status("vm-1").await.unwrap(), VmStatus::Failed);
    }

    #[tokio::test]
    async fn test_set_status_for_test_on_nonexistent_vm_is_noop() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        // Must not panic
        mgr.set_status_for_test("ghost", VmStatus::Running).await;
    }

    #[tokio::test]
    async fn test_manager_is_cloneable_and_shares_state() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        start(&mgr, "vm-1").await;
        let cloned = mgr.clone();
        // Cloned manager sees the same VMs (Arc is shared).
        assert_eq!(
            cloned.get_vm_status("vm-1").await.unwrap(),
            VmStatus::Starting
        );
        cloned.set_status_for_test("vm-1", VmStatus::Running).await;
        assert_eq!(mgr.get_vm_status("vm-1").await.unwrap(), VmStatus::Running);
    }

    // ── Concurrency ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_concurrent_start_different_vms() {
        use std::sync::Arc;
        let mgr = Arc::new(FirecrackerManager::with_config(FirecrackerConfig::stub()));
        let mut handles = vec![];

        for i in 0..20 {
            let m = mgr.clone();
            handles.push(tokio::spawn(async move {
                let result = m
                    .start_vm(
                        format!("vm-{}", i),
                        format!("app-{}", i),
                        "nginx:latest".to_string(),
                        config(),
                    )
                    .await;
                assert!(result.is_ok(), "start_vm failed for vm-{}: {:?}", i, result);
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        assert_eq!(mgr.list_vms().await.len(), 20);
    }

    #[tokio::test]
    async fn test_concurrent_start_same_vm_only_one_succeeds() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let mgr = Arc::new(FirecrackerManager::with_config(FirecrackerConfig::stub()));
        let success_count = Arc::new(AtomicUsize::new(0));
        let mut handles = vec![];

        for _ in 0..10 {
            let m = mgr.clone();
            let counter = success_count.clone();
            handles.push(tokio::spawn(async move {
                if m.start_vm(
                    "shared-vm".to_string(),
                    "app-1".to_string(),
                    "nginx".to_string(),
                    config(),
                )
                .await
                .is_ok()
                {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        assert_eq!(
            success_count.load(Ordering::SeqCst),
            1,
            "exactly one thread should have started the shared VM"
        );
        assert_eq!(mgr.list_vms().await.len(), 1);
    }

    #[tokio::test]
    async fn test_concurrent_start_and_stop_different_vms() {
        use std::sync::Arc;
        let mgr = Arc::new(FirecrackerManager::with_config(FirecrackerConfig::stub()));

        // Pre-start 10 VMs.
        for i in 0..10 {
            start(&mgr, &format!("vm-pre-{}", i)).await;
        }

        let mut handles = vec![];
        // 10 tasks start new VMs.
        for i in 10..20 {
            let m = mgr.clone();
            handles.push(tokio::spawn(async move {
                m.start_vm(
                    format!("vm-{}", i),
                    "app".to_string(),
                    "img".to_string(),
                    config(),
                )
                .await
                .unwrap();
            }));
        }
        // 10 tasks stop the pre-started VMs.
        for i in 0..10 {
            let m = mgr.clone();
            handles.push(tokio::spawn(async move {
                m.stop_vm(&format!("vm-pre-{}", i)).await.unwrap();
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        assert_eq!(mgr.list_vms().await.len(), 20);
        for i in 0..10 {
            assert_eq!(
                mgr.get_vm_status(&format!("vm-pre-{}", i)).await.unwrap(),
                VmStatus::Stopping
            );
        }
        for i in 10..20 {
            assert_eq!(
                mgr.get_vm_status(&format!("vm-{}", i)).await.unwrap(),
                VmStatus::Starting
            );
        }
    }

    #[tokio::test]
    async fn test_concurrent_reads_do_not_deadlock() {
        use std::sync::Arc;
        let mgr = Arc::new(FirecrackerManager::with_config(FirecrackerConfig::stub()));
        for i in 0..5 {
            start(&mgr, &format!("vm-{}", i)).await;
        }

        let mut handles = vec![];
        for _ in 0..20 {
            let m = mgr.clone();
            handles.push(tokio::spawn(async move {
                let _ = m.list_vms().await;
                let _ = m.get_vm_status("vm-0").await;
                let _ = m.get_vm("vm-1").await;
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
    }

    // ── wait_for_socket ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_wait_for_socket_times_out_when_file_never_appears() {
        let result = wait_for_socket(
            "/tmp/fc-nonexistent-socket-xyz-abc.sock",
            Duration::from_millis(120),
        )
        .await;
        assert!(
            matches!(result, Err(FirecrackerError::SocketTimeout(_))),
            "expected SocketTimeout, got {result:?}"
        );
    }

    #[tokio::test]
    async fn test_wait_for_socket_succeeds_immediately_when_file_exists() {
        let path = format!("/tmp/fc-wait-exists-{}.sock", uuid::Uuid::new_v4());
        tokio::fs::write(&path, b"").await.unwrap();
        let result = wait_for_socket(&path, Duration::from_millis(200)).await;
        let _ = tokio::fs::remove_file(&path).await;
        assert!(result.is_ok(), "{result:?}");
    }

    #[tokio::test]
    async fn test_wait_for_socket_succeeds_when_file_appears_later() {
        let path = format!("/tmp/fc-wait-late-{}.sock", uuid::Uuid::new_v4());
        let path2 = path.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(80)).await;
            let _ = tokio::fs::write(&path2, b"").await;
        });
        let result = wait_for_socket(&path, Duration::from_millis(500)).await;
        let _ = tokio::fs::remove_file(&path).await;
        assert!(result.is_ok(), "{result:?}");
    }

    #[tokio::test]
    async fn test_wait_for_socket_error_message_contains_path() {
        let path = "/tmp/fc-no-such-socket-abc123.sock";
        let err = wait_for_socket(path, Duration::from_millis(60))
            .await
            .unwrap_err();
        assert!(err.to_string().contains(path));
    }

    // ── fc_put ────────────────────────────────────────────────────────────────

    /// Bind a Unix socket, spawn an echo task that returns `response`, and
    /// return the socket path. The socket file exists before this returns.
    async fn spawn_mock_api(response: &'static str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let path = format!("/tmp/fc-mock-{}.sock", uuid::Uuid::new_v4());
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let path_clone = path.clone();
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf).await;
                let _ = stream.write_all(response.as_bytes()).await;
            }
            let _ = tokio::fs::remove_file(&path_clone).await;
        });
        // Yield so the spawned task enters accept() before we return.
        tokio::task::yield_now().await;
        path
    }

    #[tokio::test]
    async fn test_fc_put_returns_ok_on_204() {
        let sock = spawn_mock_api("HTTP/1.1 204 No Content\r\n\r\n").await;
        let result = fc_put(&sock, "/machine-config", r#"{"vcpu_count":1}"#).await;
        assert!(result.is_ok(), "{result:?}");
    }

    #[tokio::test]
    async fn test_fc_put_returns_ok_on_200() {
        let sock = spawn_mock_api("HTTP/1.1 200 OK\r\n\r\n").await;
        let result = fc_put(&sock, "/actions", r#"{"action_type":"InstanceStart"}"#).await;
        assert!(result.is_ok(), "{result:?}");
    }

    #[tokio::test]
    async fn test_fc_put_returns_api_error_on_400() {
        let sock = spawn_mock_api("HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n").await;
        let result = fc_put(&sock, "/boot-source", r#"{}"#).await;
        assert!(
            matches!(result, Err(FirecrackerError::ApiError { .. })),
            "expected ApiError, got {result:?}"
        );
    }

    #[tokio::test]
    async fn test_fc_put_error_contains_api_path_on_400() {
        let sock = spawn_mock_api("HTTP/1.1 400 Bad Request\r\n\r\n").await;
        let err = fc_put(&sock, "/boot-source", "{}").await.unwrap_err();
        assert!(err.to_string().contains("/boot-source"), "{err}");
    }

    #[tokio::test]
    async fn test_fc_put_returns_api_error_when_socket_missing() {
        let result = fc_put("/tmp/fc-no-socket-for-real.sock", "/test", "{}").await;
        assert!(
            matches!(result, Err(FirecrackerError::ApiError { .. })),
            "{result:?}"
        );
    }

    #[tokio::test]
    async fn test_fc_put_error_contains_api_path_on_connection_failure() {
        let err = fc_put("/tmp/fc-absent-sock.sock", "/drives/rootfs", "{}")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("/drives/rootfs"), "{err}");
    }

    // ── new error variant display ─────────────────────────────────────────────

    #[tokio::test]
    async fn test_process_error_display_contains_message() {
        let err = FirecrackerError::ProcessError("no such binary".to_string());
        assert!(err.to_string().contains("no such binary"));
    }

    #[tokio::test]
    async fn test_api_error_display_contains_path_and_message() {
        let err = FirecrackerError::ApiError {
            path: "/machine-config".to_string(),
            msg: "400 Bad Request".to_string(),
        };
        let s = err.to_string();
        assert!(s.contains("/machine-config"), "{s}");
        assert!(s.contains("400 Bad Request"), "{s}");
    }

    #[tokio::test]
    async fn test_socket_timeout_display_contains_path() {
        let err = FirecrackerError::SocketTimeout("/tmp/fc-vm-42.sock".to_string());
        assert!(err.to_string().contains("/tmp/fc-vm-42.sock"));
    }

    // ── start_vm real mode ────────────────────────────────────────────────────

    async fn real_config_bad_binary() -> (FirecrackerConfig, String) {
        // Create a real temp file to act as the rootfs so the copy step succeeds.
        let rootfs = format!("/tmp/fc-test-rootfs-{}.ext4", uuid::Uuid::new_v4());
        tokio::fs::write(&rootfs, b"fake").await.unwrap();
        let cfg = FirecrackerConfig {
            kernel_path: Some("/fake/vmlinux".to_string()),
            binary: "/nonexistent/firecracker-binary-xyz".to_string(),
            rootfs_path: rootfs.clone(),
        };
        (cfg, rootfs)
    }

    #[tokio::test]
    async fn test_start_vm_real_mode_bad_binary_returns_process_error() {
        let (cfg, rootfs) = real_config_bad_binary().await;
        let mgr = FirecrackerManager::with_config(cfg);
        let result = mgr
            .start_vm(
                "vm-bad-bin".to_string(),
                "app-1".to_string(),
                "img".to_string(),
                config(),
            )
            .await;
        let _ = tokio::fs::remove_file(&rootfs).await;

        assert!(
            matches!(result, Err(FirecrackerError::ProcessError(_))),
            "expected ProcessError, got {result:?}"
        );
        assert_eq!(
            mgr.get_vm_status("vm-bad-bin").await.unwrap(),
            VmStatus::Failed
        );
    }

    #[tokio::test]
    async fn test_start_vm_real_mode_failed_vm_has_error_message() {
        let (cfg, rootfs) = real_config_bad_binary().await;
        let mgr = FirecrackerManager::with_config(cfg);
        let _ = mgr
            .start_vm(
                "vm-err-msg".to_string(),
                "app-1".to_string(),
                "img".to_string(),
                config(),
            )
            .await;
        let _ = tokio::fs::remove_file(&rootfs).await;

        let vm = mgr.get_vm("vm-err-msg").await.unwrap();
        assert!(vm.error_message.is_some());
        assert!(!vm.error_message.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_start_vm_stub_mode_when_kernel_path_is_none() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let result = mgr
            .start_vm(
                "vm-stub".to_string(),
                "app-1".to_string(),
                "img".to_string(),
                config(),
            )
            .await;

        assert!(result.is_ok());
        assert_eq!(
            mgr.get_vm_status("vm-stub").await.unwrap(),
            VmStatus::Starting
        );
        // No process handle should have been stored.
        assert!(!mgr.has_process("vm-stub").await);
    }

    // ── stop_vm real mode ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_stop_vm_real_mode_kills_process_and_sets_stopped() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let vm_id = "vm-kill-test";

        // Insert VM state directly (mimics what start_vm does).
        {
            let mut vms = mgr.vms.write().await;
            vms.insert(
                vm_id.to_string(),
                VmInfo {
                    vm_id: vm_id.to_string(),
                    app_id: "app-1".to_string(),
                    image: "img".to_string(),
                    config: config(),
                    status: VmStatus::Running,
                    started_at: None,
                    error_message: None,
                },
            );
        }

        // Create a socket file so cleanup can be verified.
        let socket_path = format!("/tmp/fc-test-kill-{}.sock", uuid::Uuid::new_v4());
        tokio::fs::write(&socket_path, b"").await.unwrap();

        // Spawn a real long-lived process as stand-in.
        let child = tokio::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        mgr.insert_process_for_test(vm_id, child, socket_path.clone())
            .await;

        // Stop the VM.
        mgr.stop_vm(vm_id).await.unwrap();

        // Status must be Stopped (real mode: process was found and killed).
        assert_eq!(mgr.get_vm_status(vm_id).await.unwrap(), VmStatus::Stopped);

        // Process handle should have been removed from the map.
        assert!(!mgr.has_process(vm_id).await);

        // Socket file should have been deleted.
        assert!(
            !std::path::Path::new(&socket_path).exists(),
            "socket file should be cleaned up"
        );

        // Give the OS a moment, then confirm the process is gone.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let proc_alive = std::path::Path::new(&format!("/proc/{pid}")).exists();
        assert!(!proc_alive, "process {pid} should have been killed");
    }

    #[tokio::test]
    async fn test_stop_vm_stub_mode_leaves_stopping_status() {
        // No process in map → stub behaviour: status stays Stopping.
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        start(&mgr, "vm-stub-stop").await;
        mgr.stop_vm("vm-stub-stop").await.unwrap();
        assert_eq!(
            mgr.get_vm_status("vm-stub-stop").await.unwrap(),
            VmStatus::Stopping
        );
    }

    #[tokio::test]
    async fn test_stop_vm_real_mode_cleans_up_socket_even_if_already_gone() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let vm_id = "vm-no-socket";
        {
            let mut vms = mgr.vms.write().await;
            vms.insert(
                vm_id.to_string(),
                VmInfo {
                    vm_id: vm_id.to_string(),
                    app_id: "app-1".to_string(),
                    image: "img".to_string(),
                    config: config(),
                    status: VmStatus::Running,
                    started_at: None,
                    error_message: None,
                },
            );
        }
        // Socket path that does not exist — remove_file should be a no-op.
        let nonexistent_sock = "/tmp/fc-already-gone.sock".to_string();
        let child = tokio::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .unwrap();
        mgr.insert_process_for_test(vm_id, child, nonexistent_sock)
            .await;

        // Must not panic even though the socket file is absent.
        mgr.stop_vm(vm_id).await.unwrap();
        assert_eq!(mgr.get_vm_status(vm_id).await.unwrap(), VmStatus::Stopped);
    }

    #[tokio::test]
    async fn test_setup_tap_name_generation() {
        let mgr = FirecrackerManager::new();
        let vm_id = "test-vm-id-123456789";

        // We just test the setup_tap logic internally by calling it.
        // It might fail because of lack of permissions, but we want to see if it tries the right name.
        let result = mgr.setup_tap(vm_id).await;

        if let Err(FirecrackerError::ProcessError(msg)) = result {
            // Check if it failed because of permissions (ioctl) OR it tried the right name
            let is_permission_denied =
                msg.contains("Operation not permitted") || msg.contains("ioctl");
            let contains_tap_name = msg.contains("m-tap-test-vm-");

            assert!(
                is_permission_denied || contains_tap_name,
                "Error should be permissions related or mention tap name: {}",
                msg
            );
        }
    }

    #[tokio::test]
    async fn test_initial_log_capture() {
        let mgr = FirecrackerManager::new();
        let vm_id = "log-test-vm";

        // Our current start_vm adds an initial log entry even before spawning.
        let config = VmConfig {
            vcpus: 1,
            memory_mib: 128,
            disk_mib: 1024,
            env: HashMap::new(),
            ..Default::default()
        };

        // This will return Ok in stub mode (no kernel path) but should still add logs
        let _ = mgr
            .start_vm(
                vm_id.to_string(),
                "app-1".to_string(),
                "image".to_string(),
                config,
            )
            .await;

        let logs = mgr.get_logs(vm_id).await;
        assert!(
            !logs.is_empty(),
            "Logs should not be empty after initialization"
        );
        assert!(
            logs[0].contains("Initializing VM"),
            "First log should be initialization message"
        );
    }

    #[tokio::test]
    async fn test_ensure_volume_creates_file() {
        let mgr = FirecrackerManager::new();
        let volume_id = format!("test-vol-{}", uuid::Uuid::new_v4());
        let size_mib = 10;

        let path = mgr.ensure_volume(&volume_id, size_mib).await.unwrap();
        assert!(std::path::Path::new(&path).exists());

        let metadata = std::fs::metadata(&path).unwrap();
        assert_eq!(metadata.len(), size_mib * 1024 * 1024);

        // Clean up
        let _ = std::fs::remove_file(path);
    }
}
