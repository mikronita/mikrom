use crate::firecracker::api::{fc_patch, fc_put, wait_for_socket};
use crate::firecracker::config::{FirecrackerConfig, FirecrackerError, VmConfig, VmInfo, VmStatus};
use crate::firecracker::process::{
    CommandExecutor, RealCommandExecutor, VmDetailedInfo, VmProcess,
};

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{Mutex, RwLock};

/// Orchestrator for managing the lifecycle of Firecracker microVMs on a single host.
///
/// It handles networking, image conversion, jailer setup, and API communication
/// with the Firecracker process.
#[derive(Clone)]
pub struct FirecrackerManager {
    /// Unique identifier for this agent.
    pub agent_id: String,
    /// Thread-safe map of active microVM information.
    pub vms: Arc<RwLock<HashMap<String, VmInfo>>>,
    /// Thread-safe map of active Firecracker processes.
    pub processes: Arc<Mutex<HashMap<String, VmProcess>>>,
    /// Configuration for Firecracker and Jailer.
    pub fc_config: FirecrackerConfig,
    /// In-memory ring buffer of logs for each VM.
    pub logs: Arc<RwLock<HashMap<String, VecDeque<String>>>>,
    /// Image builder for converting OCI images to Firecracker-compatible rootfs.
    pub builder: Arc<crate::builder::ImageBuilder>,
    /// Interface for executing system commands.
    pub executor: Arc<dyn CommandExecutor>,
    /// Tracks allocated IP addresses on the host bridge.
    pub allocated_ips: Arc<tokio::sync::Mutex<std::collections::HashSet<std::net::Ipv4Addr>>>,
}

impl FirecrackerManager {
    /// Returns a list of all VMs with detailed status and PID information.
    pub async fn get_all_vms(&self) -> Vec<VmDetailedInfo> {
        let vms = self.vms.read().await;
        let processes = self.processes.lock().await;

        vms.values()
            .map(|vm| {
                let proc = processes.get(&vm.vm_id);
                let pid = proc.map(|p| p.child.id().unwrap_or(0));
                let metrics_path = proc.and_then(|p| p.metrics_path.clone());
                let socket_path = proc.map(|p| p.socket_path.clone());

                VmDetailedInfo {
                    vm_id: vm.vm_id.clone(),
                    status: vm.status,
                    error_message: vm.error_message.clone(),
                    pid,
                    ip_address: vm.config.ip_address.clone(),
                    metrics_path,
                    socket_path,
                }
            })
            .collect()
    }
    /// Create a manager whose configuration is read from environment variables.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(FirecrackerConfig::from_env())
    }

    /// Create a manager with an explicit configuration (useful for tests).
    #[must_use]
    pub fn with_config(fc_config: FirecrackerConfig) -> Self {
        let builder =
            crate::builder::ImageBuilder::new().expect("Failed to initialize Docker builder");

        let mut agent_id = uuid::Uuid::new_v4().to_string();

        // Ensure data directory exists and handle persistent agent_id
        if !fc_config.data_dir.is_empty() {
            let data_path = std::path::Path::new(&fc_config.data_dir);

            if let Err(e) = std::fs::create_dir_all(data_path) {
                tracing::error!("Failed to create data directory {:?}: {}", data_path, e);
            }

            let snapshots_path = data_path.join("snapshots");
            if let Err(e) = std::fs::create_dir_all(&snapshots_path) {
                tracing::error!(
                    "Failed to create snapshots directory {:?}: {}",
                    snapshots_path,
                    e
                );
            }

            let volumes_path = data_path.join("volumes");
            if let Err(e) = std::fs::create_dir_all(&volumes_path) {
                tracing::error!(
                    "Failed to create volumes directory {:?}: {}",
                    volumes_path,
                    e
                );
            }

            let id_path = data_path.join("agent_id.txt");
            if let Ok(id) = std::fs::read_to_string(&id_path) {
                let id = id.trim().to_string();
                if !id.is_empty() {
                    agent_id = id;
                }
            } else if let Err(e) = std::fs::write(&id_path, &agent_id) {
                tracing::error!("Failed to write agent id to {:?}: {}", id_path, e);
            }
        }

        Self {
            agent_id,
            vms: Arc::new(RwLock::new(HashMap::new())),
            processes: Arc::new(Mutex::new(HashMap::new())),
            fc_config,
            logs: Arc::new(RwLock::new(HashMap::new())),
            builder: Arc::new(builder),
            executor: Arc::new(RealCommandExecutor),
            allocated_ips: Arc::new(tokio::sync::Mutex::new(std::collections::HashSet::new())),
        }
    }

    pub fn with_executor(mut self, executor: Arc<dyn CommandExecutor>) -> Self {
        self.executor = executor;
        self
    }

    pub fn start_background_tasks(&self) {
        let self_clone = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                self_clone.run_gc().await;
            }
        });
    }

    async fn run_gc(&self) {
        tracing::debug!("Running agent garbage collector...");

        // 1. Sync processes with actual child status
        let mut processes = self.processes.lock().await;
        let mut to_remove = Vec::new();

        for (vm_id, proc) in processes.iter_mut() {
            match proc.child.try_wait() {
                Ok(Some(status)) => {
                    tracing::info!(vm_id = %vm_id, status = ?status, "Detected Firecracker process exit via GC");
                    to_remove.push(vm_id.clone());
                },
                Ok(None) => {
                    // Still running
                },
                Err(e) => {
                    tracing::error!(vm_id = %vm_id, error = %e, "Error checking Firecracker process status");
                },
            }
        }

        for vm_id in to_remove {
            if let Some(proc) = processes.remove(&vm_id) {
                if let Err(e) = tokio::fs::remove_file(&proc.socket_path).await
                    && e.kind() != std::io::ErrorKind::NotFound
                {
                    tracing::debug!("Failed to remove socket {}: {}", proc.socket_path, e);
                }
                if let Some(tap) = &proc.tap_name {
                    self.cleanup_tap(tap).await;
                }

                let mut vms = self.vms.write().await;
                if let Some(vm) = vms.get_mut(&vm_id)
                    && vm.status == VmStatus::Running
                {
                    if let Some(ip) = &vm.config.ip_address {
                        self.release_vm_ip(ip).await;
                    }
                    vm.status = VmStatus::Stopped;
                }
            }
        }

        // Drop the lock before calling cleanup_all_stale_resources to prevent deadlock
        drop(processes);

        // 2. Clean up stale resources in /tmp (agent-specific)
        self.cleanup_all_stale_resources().await;
    }

    pub async fn start_vm(
        &self,
        vm_id: String,
        app_id: String,
        image: String,
        config: VmConfig,
    ) -> Result<(), FirecrackerError> {
        // 1. Pre-check and initial state registration
        {
            let mut vms = self.vms.write().await;
            if vms.contains_key(&vm_id) {
                return Err(FirecrackerError::StartFailed(
                    "VM already exists".to_string(),
                ));
            }

            vms.insert(
                vm_id.clone(),
                VmInfo {
                    vm_id: vm_id.clone(),
                    app_id: app_id.clone(),
                    image: image.clone(),
                    config: config.clone(),
                    status: VmStatus::Starting,
                    started_at: None,
                    error_message: None,
                },
            );
        }

        // 2. Add initial log entry
        {
            let mut l = self.logs.write().await;
            l.entry(vm_id.clone())
                .or_default()
                .push_back(format!("[agent] Initializing VM {vm_id}..."));
        }

        // 3. In real mode, validate the binary exists before going async.
        if let Some(_kernel) = &self.fc_config.kernel_path {
            let binary = &self.fc_config.binary;
            if tokio::fs::metadata(binary).await.is_err() {
                let err_msg = format!("Firecracker binary not found: {binary}");
                self.set_failed(&vm_id, err_msg.clone()).await;
                return Err(FirecrackerError::ProcessError(err_msg));
            }
        }

        // 4. Spawn the heavy work in background
        let self_clone = self.clone();
        let vm_id_clone = vm_id.clone();
        let app_id_clone = app_id.clone();
        let image_clone = image.clone();
        let config_clone = config.clone();

        tracing::info!(vm_id = %vm_id, "Spawning background startup task");
        tokio::spawn(async move {
            let vid = vm_id_clone.clone();
            tracing::info!(vm_id = %vid, "Inside tokio::spawn block");
            if let Err(e) = self_clone
                .start_vm_background(vm_id_clone, app_id_clone, image_clone, config_clone)
                .await
            {
                let err_msg = format!("Failed to start VM in background: {}", e);
                tracing::error!(vm_id = %vid, error = %e, "{}", err_msg);
                self_clone.set_failed(&vid, err_msg).await;
            }
        });

        Ok(())
    }

    #[tracing::instrument(skip(self, config), fields(vm_id = %vm_id, app_id = %app_id))]
    async fn start_vm_background(
        &self,
        vm_id: String,
        app_id: String,
        image: String,
        config: VmConfig,
    ) -> Result<(), FirecrackerError> {
        tracing::info!(vm_id = %vm_id, "Background VM startup initiated");
        let kernel_path = if let Some(p) = self.fc_config.kernel_path.clone() {
            p
        } else {
            tracing::info!(vm_id = %vm_id, "Stub mode: marking as running");
            // Stub mode
            let mut vms = self.vms.write().await;
            if let Some(vm) = vms.get_mut(&vm_id) {
                vm.status = VmStatus::Running;
                vm.started_at = Some(chrono::Utc::now().timestamp());
            }
            return Ok(());
        };

        let rootfs_path = std::path::Path::new(&self.fc_config.data_dir)
            .join(format!("fc-{}-{}-rootfs.ext4", self.agent_id, vm_id))
            .to_string_lossy()
            .to_string();
        self.prepare_rootfs(&vm_id, &image, &rootfs_path, config.port)
            .await?;

        // Resolve networking: if scheduler didn't assign an IP, allocate from bridge subnet.
        let config = if config.ip_address.as_deref().unwrap_or("").is_empty() {
            match self.allocate_vm_network().await {
                Some((ip, gw, mac)) => {
                    tracing::info!(vm_id = %vm_id, ip = %ip, "Allocated IP from agent bridge subnet");
                    VmConfig {
                        ip_address: Some(ip),
                        gateway: Some(gw),
                        mac_address: Some(mac),
                        ..config
                    }
                },
                None => {
                    tracing::warn!(vm_id = %vm_id, "No available IPs in bridge subnet, starting without networking");
                    config
                },
            }
        } else {
            config
        };

        // ── Networking setup ───────────────────────────────────────────────────
        let tap_name = if config.ip_address.is_some() {
            Some(self.setup_tap(&vm_id).await?)
        } else {
            None
        };

        // ── Jailer setup (if enabled) ──────────────────────────────────────────
        let fc_binary = &self.fc_config.binary;
        let (exec_binary, exec_args, socket_path, chroot_dir) = if self.fc_config.use_jailer {
            self.setup_jailer(&vm_id, &kernel_path, &rootfs_path)
                .await?
        } else {
            let socket_path = std::path::Path::new(&self.fc_config.data_dir)
                .join(format!("fc-{}-{}.sock", self.agent_id, vm_id))
                .to_string_lossy()
                .to_string();
            // Remove any stale socket from a previous run.
            if let Err(e) = tokio::fs::remove_file(&socket_path).await
                && e.kind() != std::io::ErrorKind::NotFound
            {
                tracing::debug!("Failed to remove stale socket {}: {}", socket_path, e);
            }

            (
                fc_binary.clone(),
                vec!["--api-sock".to_string(), socket_path.clone()],
                socket_path,
                None,
            )
        };

        // Spawn the Firecracker process.
        let mut child = match tokio::process::Command::new(&exec_binary)
            .args(&exec_args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let err_msg =
                    format!("Failed to spawn firecracker process (binary: {exec_binary}): {e}");
                tracing::error!("{}", err_msg);
                if let Some(tap) = &tap_name {
                    self.cleanup_tap(tap).await;
                }
                if let Some(chroot) = chroot_dir
                    && let Err(e) = tokio::fs::remove_dir_all(&chroot).await
                {
                    tracing::error!(
                        "Failed to remove chroot directory {} on failure: {}",
                        chroot,
                        e
                    );
                }
                self.set_failed(&vm_id, err_msg.clone()).await;
                return Err(FirecrackerError::ProcessError(err_msg));
            },
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
                    Ok(Some(line)) = stderr_lines.next_line() => Some(format!("[stderr] {line}")),
                    else => None,
                };

                if let Some(l) = line {
                    let mut logs = logs_clone.write().await;
                    let vm_logs = logs
                        .entry(vm_id_clone.clone())
                        .or_insert_with(|| VecDeque::with_capacity(1000));
                    if vm_logs.len() >= 1000 {
                        vm_logs.pop_front();
                    }
                    vm_logs.push_back(l);
                } else {
                    break;
                }
            }
        });

        // Wait for the API socket to appear (up to 10 s if using jailer as it takes longer).
        let wait_timeout = if chroot_dir.is_some() {
            Duration::from_secs(10)
        } else {
            Duration::from_secs(5)
        };

        if let Err(e) = wait_for_socket(&socket_path, wait_timeout).await {
            let _ = child.kill().await;
            if let Some(tap) = &tap_name {
                self.cleanup_tap(tap).await;
            }
            if let Some(chroot) = chroot_dir
                && let Err(e) = tokio::fs::remove_dir_all(&chroot).await
            {
                tracing::error!(
                    "Failed to clean up chroot directory {} on failure: {}",
                    chroot,
                    e
                );
            }
            self.set_failed(&vm_id, e.to_string()).await;
            return Err(e);
        }

        // ── Metrics setup ──────────────────────────────────────────────────────
        let (metrics_host_path, metrics_api_path) = if let Some(chroot) = &chroot_dir {
            let host_path = format!("{chroot}/root/metrics.json");
            // Create empty file and set permissions for jailer
            tokio::fs::write(&host_path, b"").await.map_err(|e| {
                FirecrackerError::ProcessError(format!("Failed to create metrics file: {e}"))
            })?;
            self.recursive_chown(
                &host_path,
                self.fc_config.jailer_uid,
                self.fc_config.jailer_gid,
            )
            .await?;
            (Some(host_path), "/metrics.json".to_string())
        } else {
            let host_path = std::path::Path::new(&self.fc_config.data_dir)
                .join(format!("fc-{}-{}-metrics.json", self.agent_id, vm_id))
                .to_string_lossy()
                .to_string();
            (Some(host_path.clone()), host_path)
        };

        let metrics_config = serde_json::json!({
            "metrics_path": metrics_api_path
        })
        .to_string();

        // Configure metrics API
        if let Err(e) = fc_put(&socket_path, "/metrics", &metrics_config).await {
            tracing::warn!(vm_id = %vm_id, "Failed to configure metrics: {e}");
        }

        // Check if we have a snapshot to restore from
        let snapshot_dir = std::path::Path::new(&self.fc_config.data_dir).join("snapshots");
        let snapshot_path = format!("{}/{vm_id}.snapshot", snapshot_dir.display());
        let mem_path = format!("{}/{vm_id}.mem", snapshot_dir.display());
        let has_snapshot = tokio::fs::metadata(&snapshot_path).await.is_ok()
            && tokio::fs::metadata(&mem_path).await.is_ok();

        if has_snapshot {
            tracing::info!(vm_id = %vm_id, "Found snapshot, restoring VM...");

            // 1. Prepare snapshot paths for API
            let (load_snapshot_path, load_mem_path) = if let Some(chroot) = &chroot_dir {
                let chroot_snap_path = format!("{chroot}/root/vm.snapshot");
                let chroot_mem_path = format!("{chroot}/root/vm.mem");

                self.ensure_file_at(&snapshot_path, &chroot_snap_path)
                    .await?;
                self.ensure_file_at(&mem_path, &chroot_mem_path).await?;

                self.recursive_chown(
                    &chroot_snap_path,
                    self.fc_config.jailer_uid,
                    self.fc_config.jailer_gid,
                )
                .await?;
                self.recursive_chown(
                    &chroot_mem_path,
                    self.fc_config.jailer_uid,
                    self.fc_config.jailer_gid,
                )
                .await?;

                ("/vm.snapshot".to_string(), "/vm.mem".to_string())
            } else {
                (snapshot_path, mem_path)
            };

            // 2. Load snapshot
            let load_body = serde_json::json!({
                "snapshot_path": load_snapshot_path,
                "mem_file_path": load_mem_path,
                "resume": true
            })
            .to_string();

            // Note: Networking and drives must be configured similarly or via snapshot.
            // For now, we assume simple restoration.
            if let Err(e) = fc_put(&socket_path, "/snapshot/load", &load_body).await {
                tracing::error!(
                    vm_id = %vm_id,
                    "Failed to load snapshot: {}. Falling back to normal boot.",
                    e
                );
            } else {
                let mut processes = self.processes.lock().await;
                processes.insert(
                    vm_id.clone(),
                    VmProcess {
                        vm_id: vm_id.clone(),
                        child,
                        socket_path: socket_path.to_string(),
                        metrics_path: metrics_host_path.clone(),
                        tap_name,
                        log_task,
                        chroot_dir,
                    },
                );

                let mut vms = self.vms.write().await;
                if let Some(vm) = vms.get_mut(&vm_id) {
                    vm.status = VmStatus::Running;
                }
                return Ok(());
            }
        }
        // ── Normal Boot Sequence (if no snapshot or restoration failed) ────────
        // Configure machine resources.
        let machine_config = serde_json::json!({
            "vcpu_count": config.vcpus,
            "mem_size_mib": config.memory_mib,
            "smt": false,
            "track_dirty_pages": false
        })
        .to_string();

        let mut boot_args =
            "console=ttyS0 reboot=k panic=1 pci=off nomodules rw root=/dev/vda init=/mikrom-init"
                .to_string();

        if let (Some(ip), Some(gw)) = (&config.ip_address, &config.gateway) {
            let mask = config.netmask.as_deref().unwrap_or("255.255.255.0");
            boot_args.push_str(&format!(" ip={ip}::{gw}:{mask}::eth0:off"));
        }

        let kernel_api_path = if chroot_dir.is_some() {
            "/vmlinux.bin".to_string()
        } else {
            kernel_path.clone()
        };

        let boot_source = serde_json::json!({
            "kernel_image_path": kernel_api_path,
            "boot_args": boot_args
        })
        .to_string();

        let rootfs_api_path = if chroot_dir.is_some() {
            "/rootfs.ext4".to_string()
        } else {
            rootfs_path.clone()
        };

        let drives = serde_json::json!({
            "drive_id": "rootfs",
            "path_on_host": rootfs_api_path,
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
        let chroot_dir_clone = chroot_dir.clone();
        let socket_path_clone = socket_path.clone();

        let res = (async {
            fc_put(&socket_path_clone, "/machine-config", &machine_config).await?;
            fc_put(&socket_path_clone, "/boot-source", &boot_source).await?;
            fc_put(&socket_path_clone, "/drives/rootfs", &drives).await?;

            if let Some(net_json) = &network_interface {
                fc_put(&socket_path_clone, "/network-interfaces/eth0", net_json).await?;
            }

            // ── Attach additional volumes ──────────────────────────────────────────
            for vol in &config.volumes {
                let vol_path = self.ensure_volume(&vol.volume_id, vol.size_mib).await?;

                let vol_api_path = if let Some(chroot) = &chroot_dir_clone {
                    let vol_filename = format!("{}.ext4", vol.volume_id);
                    let chroot_vol_path = format!("{chroot}/root/{vol_filename}");
                    self.ensure_file_at(&vol_path, &chroot_vol_path).await?;
                    self.recursive_chown(
                        &chroot_vol_path,
                        self.fc_config.jailer_uid,
                        self.fc_config.jailer_gid,
                    )
                    .await?;
                    format!("/{vol_filename}")
                } else {
                    vol_path
                };

                let drive_json = serde_json::json!({
                    "drive_id": vol.volume_id,
                    "path_on_host": vol_api_path,
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
            Ok::<(), FirecrackerError>(())
        })
        .await;

        if let Err(e) = res {
            tracing::error!(vm_id = %vm_id, "Firecracker API configuration failed: {}", e);
            let _ = child.kill().await;
            if let Some(tap) = &tap_name {
                self.cleanup_tap(tap).await;
            }
            if let Some(chroot) = chroot_dir
                && let Err(e) = tokio::fs::remove_dir_all(&chroot).await
            {
                tracing::error!(
                    "Failed to clean up chroot directory {} on failure: {}",
                    chroot,
                    e
                );
            }
            self.set_failed(&vm_id, e.to_string()).await;
            return Err(e);
        }

        // VM is booting — mark as Running and store the process handle.
        {
            let mut vms = self.vms.write().await;
            if let Some(vm) = vms.get_mut(&vm_id) {
                vm.status = VmStatus::Running;
                vm.started_at = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_secs() as i64),
                );
            }
        }
        self.processes.lock().await.insert(
            vm_id.clone(),
            VmProcess {
                vm_id: vm_id.clone(),
                child,
                socket_path: socket_path.to_string(),
                metrics_path: metrics_host_path,
                tap_name,
                log_task,
                chroot_dir,
            },
        );

        tracing::info!(vm_id = %vm_id, "Firecracker VM successfully started");
        Ok(())
    }

    async fn prepare_rootfs(
        &self,
        vm_id: &str,
        image: &str,
        rootfs_path: &str,
        port: u32,
    ) -> Result<(), FirecrackerError> {
        tracing::info!(vm_id = %vm_id, rootfs_path = %rootfs_path, "Preparing rootfs");
        let image_path = std::path::Path::new(image);
        let dst_path = std::path::Path::new(rootfs_path);

        if image_path.is_absolute() {
            if !image_path.exists() {
                let err = format!("Local image rootfs not found at absolute path: {image}");
                return Err(FirecrackerError::ProcessError(err));
            }
            tracing::info!(vm_id = %vm_id, src = %image, dst = %rootfs_path, "Linking local rootfs...");
            self.ensure_file_at(image, rootfs_path).await?;
        } else if image_path.exists() {
            self.ensure_file_at(image, rootfs_path).await?;
        } else {
            tracing::info!(
                vm_id = %vm_id,
                image = %image,
                "Image not found as local file, attempting docker pull/convert"
            );
            self.builder
                .docker_to_ext4(image, dst_path, port)
                .await
                .map_err(|e| {
                    FirecrackerError::ProcessError(format!("Image builder failed: {e}"))
                })?;
        }
        Ok(())
    }

    #[tracing::instrument(skip(self), fields(vm_id = %vm_id))]
    pub async fn stop_vm(&self, vm_id: &str) -> Result<(), FirecrackerError> {
        {
            let mut vms = self.vms.write().await;
            match vms.get_mut(vm_id) {
                Some(vm) => vm.status = VmStatus::Stopping,
                None => return Err(FirecrackerError::VmNotFound(vm_id.to_string())),
            }
        }

        self.logs.write().await.remove(vm_id);

        if let Some(mut proc) = self.processes.lock().await.remove(vm_id) {
            proc.log_task.abort();
            let _ = proc.child.kill().await;
            let _ = proc.child.wait().await;
            if let Err(e) = tokio::fs::remove_file(&proc.socket_path).await
                && e.kind() != std::io::ErrorKind::NotFound
            {
                tracing::debug!("Failed to remove socket {}: {}", proc.socket_path, e);
            }

            let rootfs_path = std::path::Path::new(&self.fc_config.data_dir)
                .join(format!("fc-{}-{}-rootfs.ext4", self.agent_id, vm_id));
            if let Err(e) = tokio::fs::remove_file(&rootfs_path).await
                && e.kind() != std::io::ErrorKind::NotFound
            {
                tracing::debug!("Failed to remove rootfs {:?}: {}", rootfs_path, e);
            }

            let snapshot_dir = std::path::Path::new(&self.fc_config.data_dir).join("snapshots");
            let snap_path = snapshot_dir.join(format!("{vm_id}.snapshot"));
            let mem_path = snapshot_dir.join(format!("{vm_id}.mem"));

            if let Err(e) = tokio::fs::remove_file(&snap_path).await
                && e.kind() != std::io::ErrorKind::NotFound
            {
                tracing::debug!("Failed to remove snapshot {:?}: {}", snap_path, e);
            }
            if let Err(e) = tokio::fs::remove_file(&mem_path).await
                && e.kind() != std::io::ErrorKind::NotFound
            {
                tracing::debug!("Failed to remove memory file {:?}: {}", mem_path, e);
            }

            if let Some(chroot) = proc.chroot_dir {
                tracing::info!(vm_id = %vm_id, chroot_dir = %chroot, "Cleaning up jailer chroot");
                if let Err(e) = tokio::fs::remove_dir_all(&chroot).await {
                    tracing::error!("Failed to remove chroot directory {}: {}", chroot, e);
                }
            }

            if let Some(tap) = &proc.tap_name {
                self.cleanup_tap(tap).await;
            }

            let mut vms = self.vms.write().await;
            if let Some(vm) = vms.get_mut(vm_id) {
                if let Some(ip) = &vm.config.ip_address {
                    self.release_vm_ip(ip).await;
                }
                vm.status = VmStatus::Stopped;
            }
        }

        Ok(())
    }

    #[tracing::instrument(skip(self), fields(vm_id = %vm_id))]
    pub async fn pause_vm(&self, vm_id: &str) -> Result<(), FirecrackerError> {
        let processes = self.processes.lock().await;
        let proc = processes
            .get(vm_id)
            .ok_or_else(|| FirecrackerError::VmNotFound(vm_id.to_string()))?;

        tracing::info!(vm_id = %vm_id, "Pausing VM and creating snapshot...");

        let pause_body = serde_json::json!({ "state": "Paused" }).to_string();
        fc_patch(&proc.socket_path, "/vm", &pause_body).await?;

        let snapshot_dir = std::path::Path::new(&self.fc_config.data_dir).join("snapshots");
        tokio::fs::create_dir_all(&snapshot_dir)
            .await
            .map_err(|e| {
                FirecrackerError::ProcessError(format!("Failed to create snapshots dir: {e}"))
            })?;

        let snapshot_path = format!("{}/{vm_id}.snapshot", snapshot_dir.display());
        let mem_path = format!("{}/{vm_id}.mem", snapshot_dir.display());

        let snapshot_body = serde_json::json!({
            "snapshot_type": "Full",
            "snapshot_path": snapshot_path,
            "mem_file_path": mem_path
        })
        .to_string();

        fc_put(&proc.socket_path, "/snapshot/create", &snapshot_body).await?;

        let mut vms = self.vms.write().await;
        if let Some(vm) = vms.get_mut(vm_id) {
            vm.status = VmStatus::Paused;
        }

        tracing::info!(vm_id = %vm_id, "VM paused successfully");
        Ok(())
    }

    #[tracing::instrument(skip(self), fields(vm_id = %vm_id))]
    pub async fn resume_vm(&self, vm_id: &str) -> Result<(), FirecrackerError> {
        let processes = self.processes.lock().await;
        if let Some(proc) = processes.get(vm_id) {
            let resume_body = serde_json::json!({ "state": "Resumed" }).to_string();
            fc_patch(&proc.socket_path, "/vm", &resume_body).await?;

            let mut vms = self.vms.write().await;
            if let Some(vm) = vms.get_mut(vm_id) {
                vm.status = VmStatus::Running;
            }
            return Ok(());
        }

        tracing::info!(vm_id = %vm_id, "Process missing for resume, attempting restart from snapshot...");
        drop(processes);

        let vm_info = self
            .get_vm(vm_id)
            .await
            .ok_or_else(|| FirecrackerError::VmNotFound(vm_id.to_string()))?;

        let config = VmConfig {
            ..Default::default()
        };

        self.start_vm(
            vm_id.to_string(),
            vm_info.app_id.clone(),
            vm_info.image.clone(),
            config,
        )
        .await?;
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
            .map(|logs| logs.iter().cloned().collect())
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
        let vol_dir = format!("{}/volumes", self.fc_config.data_dir);
        tokio::fs::create_dir_all(&vol_dir).await.map_err(|e| {
            FirecrackerError::ProcessError(format!("Failed to create volumes dir: {e}"))
        })?;

        let vol_path = format!("{vol_dir}/{volume_id}.ext4");
        if tokio::fs::metadata(&vol_path).await.is_err() {
            let file = tokio::fs::File::create(&vol_path).await.map_err(|e| {
                FirecrackerError::ProcessError(format!("Failed to create volume file: {e}"))
            })?;
            file.set_len(size_mib * 1024 * 1024).await.map_err(|e| {
                FirecrackerError::ProcessError(format!("Failed to set volume size: {e}"))
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

    pub async fn cleanup_all_stale_resources(&self) {
        tracing::info!(
            agent_id = %self.agent_id,
            data_dir = %self.fc_config.data_dir,
            "Cleaning up stale Firecracker resources..."
        );
        let prefix = format!("fc-{}-", self.agent_id);

        let active_vm_ids: std::collections::HashSet<String> = {
            let processes = self.processes.lock().await;
            let vms = self.vms.read().await;
            let mut ids: std::collections::HashSet<String> = processes.keys().cloned().collect();
            // Also protect VMs that are currently starting
            for id in vms.keys() {
                ids.insert(id.clone());
            }
            ids
        };

        if let Ok(mut entries) = tokio::fs::read_dir(&self.fc_config.data_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Ok(file_name) = entry.file_name().into_string() {
                    if !file_name.starts_with(&prefix) {
                        continue;
                    }

                    // Check for exact VM ID match in filename to avoid false positives
                    let mut is_active = false;
                    for vm_id in &active_vm_ids {
                        let expected_socket = format!("{prefix}{vm_id}.sock");
                        let expected_rootfs = format!("{prefix}{vm_id}-rootfs.ext4");
                        let expected_metrics = format!("{prefix}{vm_id}-metrics.json");

                        if file_name == expected_socket
                            || file_name == expected_rootfs
                            || file_name == expected_metrics
                        {
                            is_active = true;
                            break;
                        }
                    }

                    if !is_active
                        && (file_name.ends_with(".sock")
                            || file_name.ends_with("-rootfs.ext4")
                            || file_name.ends_with("-metrics.json"))
                    {
                        let path = entry.path();
                        tracing::debug!("Removing stale file: {:?}", path);
                        if let Err(e) = tokio::fs::remove_file(&path).await
                            && e.kind() != std::io::ErrorKind::NotFound
                        {
                            tracing::debug!("Failed to remove stale file {:?}: {}", path, e);
                        }
                    }
                }
            }
        }
    }

    fn get_bridge_config(&self) -> (String, String) {
        let env_ip = std::env::var("BRIDGE_IP").ok();
        Self::resolve_bridge_config(env_ip)
    }

    fn resolve_bridge_config(env_ip: Option<String>) -> (String, String) {
        let bridge_name = "mikrom-br0";
        let bridge_ip = env_ip.unwrap_or_else(|| "10.0.0.1/8".to_string());
        (bridge_name.to_string(), bridge_ip)
    }

    fn parse_bridge_subnet(&self) -> (std::net::Ipv4Addr, std::net::Ipv4Addr, u32) {
        let (_, bridge_cidr) = self.get_bridge_config();
        let (ip_str, prefix_str) = bridge_cidr.split_once('/').unwrap_or((&bridge_cidr, "24"));
        let prefix: u32 = prefix_str.trim().parse().unwrap_or(24);
        let gateway: std::net::Ipv4Addr = ip_str
            .trim()
            .parse()
            .unwrap_or(std::net::Ipv4Addr::new(10, 0, 0, 1));
        let mask = if prefix == 0 {
            0u32
        } else {
            !0u32 << (32 - prefix)
        };
        let base = std::net::Ipv4Addr::from(u32::from(gateway) & mask);
        (gateway, base, prefix)
    }

    async fn allocate_vm_network(&self) -> Option<(String, String, String)> {
        let (gateway, base, prefix) = self.parse_bridge_subnet();
        let host_count = (1u32 << (32 - prefix)).saturating_sub(2);
        let base_u32 = u32::from(base);
        let gw_u32 = u32::from(gateway);

        let mut allocated = self.allocated_ips.lock().await;
        for offset in 2..=host_count {
            let candidate = std::net::Ipv4Addr::from(base_u32 + offset);
            if u32::from(candidate) == gw_u32 {
                continue;
            }
            if !allocated.contains(&candidate) {
                allocated.insert(candidate);
                let o = candidate.octets();
                let mac = format!("AA:FC:{:02X}:{:02X}:{:02X}:{:02X}", o[0], o[1], o[2], o[3]);
                return Some((candidate.to_string(), gateway.to_string(), mac));
            }
        }
        None
    }

    async fn release_vm_ip(&self, ip_str: &str) {
        if let Ok(ip) = ip_str.parse::<std::net::Ipv4Addr>() {
            self.allocated_ips.lock().await.remove(&ip);
        }
    }

    pub async fn init_network(&self) -> Result<(), FirecrackerError> {
        let (bridge_name, bridge_ip) = self.get_bridge_config();

        tracing::info!(
            "Initializing network bridge {} with IP {}",
            bridge_name,
            bridge_ip
        );

        let _ = tokio::process::Command::new("ip")
            .args(["link", "add", "name", &bridge_name, "type", "bridge"])
            .output()
            .await;

        let _ = tokio::process::Command::new("ip")
            .args(["addr", "add", &bridge_ip, "dev", &bridge_name])
            .output()
            .await;

        tokio::process::Command::new("ip")
            .args(["link", "set", "dev", &bridge_name, "up"])
            .output()
            .await
            .map_err(|e| {
                FirecrackerError::ProcessError(format!("Failed to bring bridge up: {e}"))
            })?;

        tokio::process::Command::new("sysctl")
            .args(["-w", "net.ipv4.ip_forward=1"])
            .output()
            .await
            .map_err(|e| {
                FirecrackerError::ProcessError(format!("Failed to enable IP forwarding: {e}"))
            })?;

        let output = tokio::process::Command::new("iptables")
            .args([
                "-t",
                "nat",
                "-A",
                "POSTROUTING",
                "-s",
                "10.0.0.0/8",
                "!",
                "-o",
                &bridge_name,
                "-j",
                "MASQUERADE",
            ])
            .output()
            .await;

        if let Ok(o) = output {
            if !o.status.success() {
                tracing::warn!(
                    "Failed to setup iptables MASQUERADE: {}",
                    String::from_utf8_lossy(&o.stderr)
                );
            }
        } else if let Err(e) = output {
            tracing::warn!("Error running iptables: {}", e);
        }

        Ok(())
    }

    async fn setup_tap(&self, vm_id: &str) -> Result<String, FirecrackerError> {
        let tap_name = format!("m-tap-{}", &vm_id[..8]);

        let _ = tokio::process::Command::new("ip")
            .args(["link", "del", &tap_name])
            .output()
            .await;

        let output = tokio::process::Command::new("ip")
            .args([
                "tuntap",
                "add",
                "dev",
                &tap_name,
                "mode",
                "tap",
                "user",
                &self.fc_config.jailer_uid.to_string(),
            ])
            .output()
            .await
            .map_err(|e| FirecrackerError::ProcessError(format!("Failed to create TAP: {e}")))?;

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
            .map_err(|e| FirecrackerError::ProcessError(format!("Failed to set TAP up: {e}")))?;

        if !output.status.success() {
            return Err(FirecrackerError::ProcessError(format!(
                "Failed to set TAP up: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let _ = tokio::process::Command::new("ip")
            .args(["link", "set", &tap_name, "master", "mikrom-br0"])
            .output()
            .await;

        Ok(tap_name)
    }

    async fn cleanup_tap(&self, tap_name: &str) {
        let _ = tokio::process::Command::new("ip")
            .args(["link", "set", tap_name, "nomaster"])
            .output()
            .await;

        let _ = tokio::process::Command::new("ip")
            .args(["link", "delete", tap_name])
            .output()
            .await;
    }

    async fn setup_jailer(
        &self,
        vm_id: &str,
        kernel_host_path: &str,
        rootfs_host_path: &str,
    ) -> Result<(String, Vec<String>, String, Option<String>), FirecrackerError> {
        let exec_name = std::path::Path::new(&self.fc_config.binary)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("firecracker");

        let chroot_dir = std::path::Path::new(&self.fc_config.chroot_base)
            .join(exec_name)
            .join(vm_id);
        let root_dir = chroot_dir.join("root");
        let run_dir = root_dir.join("run");

        tokio::fs::create_dir_all(&run_dir).await.map_err(|e| {
            FirecrackerError::ProcessError(format!(
                "Failed to create jailer directory {:?}: {}",
                run_dir, e
            ))
        })?;

        let kernel_filename = "vmlinux.bin";
        let rootfs_filename = "rootfs.ext4";

        let chroot_kernel_path = root_dir.join(kernel_filename);
        let chroot_rootfs_path = root_dir.join(rootfs_filename);

        self.ensure_file_at(kernel_host_path, &chroot_kernel_path.to_string_lossy())
            .await?;

        self.ensure_file_at(rootfs_host_path, &chroot_rootfs_path.to_string_lossy())
            .await?;

        let uid = self.fc_config.jailer_uid;
        let gid = self.fc_config.jailer_gid;

        self.recursive_chown(&chroot_dir.to_string_lossy(), uid, gid)
            .await?;

        let socket_path = "/run/firecracker.socket";
        let args = vec![
            "--id".to_string(),
            vm_id.to_string(),
            "--exec-file".to_string(),
            self.fc_config.binary.clone(),
            "--uid".to_string(),
            uid.to_string(),
            "--gid".to_string(),
            gid.to_string(),
            "--chroot-base-dir".to_string(),
            self.fc_config.chroot_base.clone(),
            "--".to_string(),
            "--api-sock".to_string(),
            socket_path.to_string(),
        ];

        let host_socket_path = root_dir.join("run/firecracker.socket");

        Ok((
            self.fc_config.jailer_binary.clone(),
            args,
            host_socket_path.to_string_lossy().to_string(),
            Some(chroot_dir.to_string_lossy().to_string()),
        ))
    }

    async fn ensure_file_at(&self, src: &str, dst: &str) -> Result<(), FirecrackerError> {
        let canonical_src = tokio::fs::canonicalize(src).await.map_err(|e| {
            FirecrackerError::ProcessError(format!("Failed to resolve path {src}: {e}"))
        })?;

        if let Err(_e) = tokio::fs::hard_link(&canonical_src, dst).await {
            tokio::fs::copy(&canonical_src, dst).await.map_err(|e| {
                FirecrackerError::ProcessError(format!(
                    "Failed to copy file from {canonical_src:?} to {dst}: {e}"
                ))
            })?;
        }
        Ok(())
    }

    async fn recursive_chown(
        &self,
        path: &str,
        uid: u32,
        gid: u32,
    ) -> Result<(), FirecrackerError> {
        use std::os::unix::fs as unix_fs;
        let mut stack = vec![std::path::PathBuf::from(path)];

        while let Some(current_path) = stack.pop() {
            // Use lchown to avoid following symlinks
            unix_fs::lchown(&current_path, Some(uid), Some(gid)).map_err(|e| {
                FirecrackerError::ProcessError(format!("Failed to chown {current_path:?}: {e}"))
            })?;

            let metadata = tokio::fs::symlink_metadata(&current_path)
                .await
                .map_err(|e| {
                    FirecrackerError::ProcessError(format!(
                        "Failed to get metadata for {current_path:?}: {e}"
                    ))
                })?;

            if metadata.is_dir() {
                let mut entries = tokio::fs::read_dir(&current_path).await.map_err(|e| {
                    FirecrackerError::ProcessError(format!(
                        "Failed to read directory {current_path:?}: {e}"
                    ))
                })?;

                while let Some(entry) = entries.next_entry().await.map_err(|e| {
                    FirecrackerError::ProcessError(format!(
                        "Failed to get next entry in {current_path:?}: {e}"
                    ))
                })? {
                    stack.push(entry.path());
                }
            }
        }
        Ok(())
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
                vm_id: vm_id.to_string(),
                child,
                socket_path,
                metrics_path: None,
                tap_name: None,
                log_task,
                chroot_dir: None,
            },
        );
        let mut vms = self.vms.write().await;
        vms.insert(
            vm_id.to_string(),
            VmInfo {
                vm_id: vm_id.to_string(),
                app_id: "test-app".to_string(),
                image: "test-image".to_string(),
                status: VmStatus::Running,
                config: VmConfig::default(),
                started_at: None,
                error_message: None,
            },
        );
    }

    async fn has_process(&self, vm_id: &str) -> bool {
        self.processes.lock().await.contains_key(vm_id)
    }
}
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::get_unwrap)]
mod tests {
    use super::*;

    fn config() -> VmConfig {
        VmConfig {
            vcpus: 1,
            memory_mib: 256,
            disk_mib: 1024,
            port: 8080,
            env: Default::default(),
            ip_address: None,
            gateway: None,
            mac_address: None,
            netmask: None,
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
                "img.ext4".to_string(),
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
                "img.ext4".to_string(),
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
            port: 8080,
            env,
            ip_address: None,
            gateway: None,
            mac_address: None,
            netmask: None,
            volumes: vec![],
        };
        assert_eq!(&cfg.env["PORT"], "3000");
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
        mgr.set_status_for_test("ghost", VmStatus::Running).await;
    }

    #[tokio::test]
    async fn test_manager_is_cloneable_and_shares_state() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        start(&mgr, "vm-1").await;
        let cloned = mgr.clone();
        assert_eq!(
            cloned.get_vm_status("vm-1").await.unwrap(),
            VmStatus::Starting
        );
        cloned.set_status_for_test("vm-1", VmStatus::Running).await;
        assert_eq!(mgr.get_vm_status("vm-1").await.unwrap(), VmStatus::Running);
    }

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
                        format!("vm-{i}"),
                        format!("app-{i}"),
                        "nginx:latest".to_string(),
                        config(),
                    )
                    .await;
                assert!(result.is_ok(), "start_vm failed for vm-{i}: {result:?}");
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

        for i in 0..10 {
            start(&mgr, &format!("vm-pre-{i}")).await;
        }

        let mut handles = vec![];
        for i in 10..20 {
            let m = mgr.clone();
            handles.push(tokio::spawn(async move {
                m.start_vm(
                    format!("vm-{i}"),
                    "app".to_string(),
                    "img.ext4".to_string(),
                    config(),
                )
                .await
                .unwrap();
            }));
        }
        for i in 0..10 {
            let m = mgr.clone();
            handles.push(tokio::spawn(async move {
                m.stop_vm(&format!("vm-pre-{i}")).await.unwrap();
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        assert_eq!(mgr.list_vms().await.len(), 20);
        for i in 0..10 {
            assert_eq!(
                mgr.get_vm_status(&format!("vm-pre-{i}")).await.unwrap(),
                VmStatus::Stopping
            );
        }
        for i in 10..20 {
            assert_eq!(
                mgr.get_vm_status(&format!("vm-{i}")).await.unwrap(),
                VmStatus::Running
            );
        }
    }

    #[tokio::test]
    async fn test_concurrent_reads_do_not_deadlock() {
        use std::sync::Arc;
        let mgr = Arc::new(FirecrackerManager::with_config(FirecrackerConfig::stub()));
        for i in 0..5 {
            start(&mgr, &format!("vm-{i}")).await;
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
        let path =
            std::env::temp_dir().join(format!("fc-wait-exists-{}.sock", uuid::Uuid::new_v4()));
        tokio::fs::write(&path, b"").await.unwrap();
        let result = wait_for_socket(&path.to_string_lossy(), Duration::from_millis(200)).await;
        if let Err(e) = tokio::fs::remove_file(&path).await {
            tracing::warn!("Failed to remove test socket {:?}: {}", path, e);
        }
        assert!(result.is_ok(), "{result:?}");
    }

    #[tokio::test]
    async fn test_wait_for_socket_succeeds_when_file_appears_later() {
        let path = std::env::temp_dir().join(format!("fc-wait-late-{}.sock", uuid::Uuid::new_v4()));
        let path2 = path.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(80)).await;
            let _ = tokio::fs::write(&path2, b"").await;
        });
        let result = wait_for_socket(&path.to_string_lossy(), Duration::from_millis(500)).await;
        if let Err(e) = tokio::fs::remove_file(&path).await {
            tracing::warn!("Failed to remove test socket {:?}: {}", path, e);
        }
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

    async fn spawn_mock_api(response: &'static str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let path = std::env::temp_dir().join(format!("fc-mock-{}.sock", uuid::Uuid::new_v4()));
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let path_clone = path.clone();
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf).await;
                let _ = stream.write_all(response.as_bytes()).await;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            if let Err(e) = tokio::fs::remove_file(&path_clone).await {
                tracing::debug!("Failed to remove mock API socket {:?}: {}", path_clone, e);
            }
        });
        tokio::task::yield_now().await;
        path.to_string_lossy().to_string()
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
        let result = fc_put(&sock, "/boot-source", r"{}").await;
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

    async fn real_config_bad_binary() -> (FirecrackerConfig, String) {
        let rootfs =
            std::env::temp_dir().join(format!("fc-test-rootfs-{}.ext4", uuid::Uuid::new_v4()));
        let rootfs_str = rootfs.to_string_lossy().to_string();
        tokio::fs::write(&rootfs, b"fake").await.unwrap();
        let cfg = FirecrackerConfig {
            kernel_path: Some("/fake/vmlinux".to_string()),
            binary: "/nonexistent/firecracker-binary-xyz".to_string(),
            rootfs_path: rootfs_str.clone(),
            ..FirecrackerConfig::stub()
        };
        (cfg, rootfs_str)
    }

    #[tokio::test]
    async fn test_start_vm_real_mode_bad_binary_returns_process_error() {
        let (cfg, rootfs) = real_config_bad_binary().await;
        let mgr = FirecrackerManager::with_config(cfg);
        let result = mgr
            .start_vm(
                "vm-bad-bin".to_string(),
                "app-1".to_string(),
                "img.ext4".to_string(),
                config(),
            )
            .await;
        if let Err(e) = tokio::fs::remove_file(&rootfs).await {
            tracing::warn!("Failed to remove test rootfs {:?}: {}", rootfs, e);
        }

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
        let vm_id = "vm-fail-msg";
        let _result = mgr
            .start_vm(
                vm_id.to_string(),
                "app-1".to_string(),
                "img.ext4".to_string(),
                config(),
            )
            .await;

        if let Err(e) = tokio::fs::remove_file(&rootfs).await {
            tracing::warn!("Failed to remove test rootfs {:?}: {}", rootfs, e);
        }
        let vm = mgr.get_vm(vm_id).await.unwrap();
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
                "img.ext4".to_string(),
                config(),
            )
            .await;

        assert!(result.is_ok());
        assert_eq!(
            mgr.get_vm_status("vm-stub").await.unwrap(),
            VmStatus::Starting
        );
        assert!(!mgr.has_process("vm-stub").await);
    }

    #[tokio::test]
    async fn test_stop_vm_real_mode_kills_process_and_sets_stopped() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let vm_id = "vm-kill-test";

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

        let socket_path =
            std::env::temp_dir().join(format!("fc-test-kill-{}.sock", uuid::Uuid::new_v4()));
        let socket_path_str = socket_path.to_string_lossy().to_string();
        tokio::fs::write(&socket_path, b"").await.unwrap();

        let child = tokio::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        mgr.insert_process_for_test(vm_id, child, socket_path_str.clone())
            .await;

        mgr.stop_vm(vm_id).await.unwrap();
        assert_eq!(mgr.get_vm_status(vm_id).await.unwrap(), VmStatus::Stopped);
        assert!(!mgr.has_process(vm_id).await);
        assert!(!socket_path.exists(), "socket file should be cleaned up");

        tokio::time::sleep(Duration::from_millis(50)).await;
        let proc_alive = std::path::Path::new(&format!("/proc/{pid}")).exists();
        assert!(!proc_alive, "process {pid} should have been killed");
    }

    #[tokio::test]
    async fn test_stop_vm_stub_mode_leaves_stopping_status() {
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
        let nonexistent_sock = "/tmp/fc-already-gone.sock".to_string();
        let child = tokio::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .unwrap();
        mgr.insert_process_for_test(vm_id, child, nonexistent_sock)
            .await;

        mgr.stop_vm(vm_id).await.unwrap();
        assert_eq!(mgr.get_vm_status(vm_id).await.unwrap(), VmStatus::Stopped);
    }

    #[tokio::test]
    async fn test_setup_tap_name_generation() {
        let mgr = FirecrackerManager::new();
        let vm_id = "test-vm-id-123456789";
        let result = mgr.setup_tap(vm_id).await;

        if let Err(FirecrackerError::ProcessError(msg)) = result {
            let is_permission_denied =
                msg.contains("Operation not permitted") || msg.contains("ioctl");
            let contains_tap_name = msg.contains("m-tap-test-vm-");

            assert!(
                is_permission_denied || contains_tap_name,
                "Error should be permissions related or mention tap name: {msg}"
            );
        }
    }

    #[tokio::test]
    async fn test_ensure_file_at_links_or_copies() {
        let temp_dir =
            std::env::temp_dir().join(format!("ensure-file-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let src = temp_dir.join("src");
        let dst = temp_dir.join("dst");
        tokio::fs::write(&src, b"data").await.unwrap();

        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        mgr.ensure_file_at(&src.to_string_lossy(), &dst.to_string_lossy())
            .await
            .expect("ensure_file_at failed");

        let content = tokio::fs::read_to_string(&dst).await.unwrap();
        assert_eq!(content, "data");

        let src_meta = tokio::fs::metadata(&src).await.unwrap();
        let dst_meta = tokio::fs::metadata(&dst).await.unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(
                src_meta.ino(),
                dst_meta.ino(),
                "Files should be hard linked (same inode)"
            );
        }

        if let Err(e) = tokio::fs::remove_dir_all(&temp_dir).await {
            tracing::error!("Failed to clean up test directory {:?}: {}", temp_dir, e);
        }
    }

    #[tokio::test]
    async fn test_recursive_chown_traversal() {
        let temp_dir = std::env::temp_dir().join(format!("chown-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(temp_dir.join("sub/dir"))
            .await
            .unwrap();
        tokio::fs::write(temp_dir.join("file1"), b"").await.unwrap();
        tokio::fs::write(temp_dir.join("sub/file2"), b"")
            .await
            .unwrap();

        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let meta = tokio::fs::metadata(&temp_dir).await.unwrap();
            let uid = meta.uid();
            let gid = meta.gid();

            mgr.recursive_chown(&temp_dir.to_string_lossy(), uid, gid)
                .await
                .expect("recursive_chown failed");
        }

        if let Err(e) = tokio::fs::remove_dir_all(&temp_dir).await {
            tracing::error!("Failed to clean up test directory {:?}: {}", temp_dir, e);
        }
    }

    #[tokio::test]
    async fn test_jailer_setup_logic() {
        let mut cfg = FirecrackerConfig::stub();
        cfg.use_jailer = true;
        cfg.chroot_base = std::env::temp_dir()
            .join(format!("jailer-test-{}", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .to_string();

        let temp_dir = std::env::temp_dir().join(format!("fc-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let kernel_path = temp_dir.join("vmlinux");
        let rootfs_path = temp_dir.join("rootfs");
        tokio::fs::write(&kernel_path, b"kernel").await.unwrap();
        tokio::fs::write(&rootfs_path, b"rootfs").await.unwrap();

        let mgr = FirecrackerManager::with_config(cfg.clone());

        let result = mgr
            .setup_jailer(
                "vm-jail-1",
                &kernel_path.to_string_lossy(),
                &rootfs_path.to_string_lossy(),
            )
            .await;

        if let Err(e) = tokio::fs::remove_dir_all(&temp_dir).await {
            tracing::error!("Failed to clean up test directory {:?}: {}", temp_dir, e);
        }

        match result {
            Ok((bin, args, socket, chroot)) => {
                assert_eq!(bin, cfg.jailer_binary);
                assert!(args.contains(&"vm-jail-1".to_string()));
                assert!(args.contains(&cfg.chroot_base));
                assert!(socket.contains("vm-jail-1"));
                assert!(chroot.is_some());

                let chroot_val = chroot.unwrap();
                assert!(chroot_val.contains(&cfg.chroot_base));

                assert!(std::path::Path::new(&format!("{chroot_val}/root/run")).exists());
                assert!(std::path::Path::new(&format!("{chroot_val}/root/vmlinux.bin")).exists());
                assert!(std::path::Path::new(&format!("{chroot_val}/root/rootfs.ext4")).exists());

                if let Err(e) = tokio::fs::remove_dir_all(&cfg.chroot_base).await {
                    tracing::error!(
                        "Failed to clean up jailer test base {:?}: {}",
                        cfg.chroot_base,
                        e
                    );
                }
            },
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("chown failed") || err_msg.contains("Operation not permitted") {
                    println!("setup_jailer failed as expected (no root for chown): {err_msg}");
                } else {
                    panic!("setup_jailer failed unexpectedly: {err_msg}");
                }
                if let Err(e) = tokio::fs::remove_dir_all(&cfg.chroot_base).await {
                    tracing::error!(
                        "Failed to clean up jailer test base {:?}: {}",
                        cfg.chroot_base,
                        e
                    );
                }
            },
        }
    }

    #[tokio::test]
    async fn test_gc_cleans_finished_processes() {
        let temp_uuid = uuid::Uuid::new_v4();
        let temp_dir = std::env::temp_dir().join(format!("mikrom-test-gc-{}", temp_uuid));
        let mut cfg = FirecrackerConfig::stub();
        cfg.data_dir = temp_dir.to_string_lossy().to_string();

        let mgr = FirecrackerManager::with_config(cfg);
        let vm_id = "gc-test";

        let child = tokio::process::Command::new("true").spawn().unwrap();

        let socket = std::path::Path::new(&mgr.fc_config.data_dir)
            .join(format!("fc-{}-gc.sock", mgr.agent_id));
        tokio::fs::write(&socket, b"").await.unwrap();

        mgr.insert_process_for_test(vm_id, child, socket.to_string_lossy().to_string())
            .await;

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        mgr.run_gc().await;

        assert!(!mgr.processes.lock().await.contains_key(vm_id));
        assert_eq!(mgr.get_vm_status(vm_id).await.unwrap(), VmStatus::Stopped);
        assert!(!socket.exists(), "Socket should be cleaned up by GC");

        if let Err(e) = tokio::fs::remove_dir_all(&temp_dir).await {
            tracing::error!("Failed to clean up test directory {:?}: {}", temp_dir, e);
        }
    }

    #[tokio::test]
    async fn test_agent_id_isolation_vms() {
        let mgr1 = FirecrackerManager::new();
        let mgr2 = FirecrackerManager::new();

        assert_ne!(
            mgr1.agent_id, mgr2.agent_id,
            "Each manager should have a unique agent_id"
        );
    }

    #[tokio::test]
    async fn test_initial_log_capture() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let vm_id = "log-test-vm";

        let config = VmConfig {
            vcpus: 1,
            memory_mib: 128,
            disk_mib: 1024,
            env: HashMap::new(),
            ..Default::default()
        };
        mgr.start_vm(
            vm_id.to_string(),
            "app-1".to_string(),
            "image".to_string(),
            config,
        )
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;

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
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let volume_id = format!("test-vol-{}", uuid::Uuid::new_v4());
        let size_mib = 10;

        let path = mgr.ensure_volume(&volume_id, size_mib).await.unwrap();
        let path_buf = std::path::PathBuf::from(&path);
        assert!(path_buf.exists());

        let metadata = std::fs::metadata(&path).unwrap();
        assert_eq!(metadata.len(), size_mib * 1024 * 1024);

        if let Err(e) = std::fs::remove_file(path) {
            tracing::error!("Failed to clean up test volume file: {}", e);
        }
    }

    #[tokio::test]
    async fn test_cleanup_stale_resources() {
        let temp_uuid = uuid::Uuid::new_v4();
        let temp_dir = std::env::temp_dir().join(format!("mikrom-test-stale-{}", temp_uuid));
        let mut cfg = FirecrackerConfig::stub();
        cfg.data_dir = temp_dir.to_string_lossy().to_string();

        let mgr = FirecrackerManager::with_config(cfg);
        let data_dir = std::path::Path::new(&mgr.fc_config.data_dir);

        let socket = data_dir.join(format!("fc-{}-stale-test.sock", mgr.agent_id));
        let rootfs = data_dir.join(format!("fc-{}-stale-test-rootfs.ext4", mgr.agent_id));

        tokio::fs::write(&socket, b"").await.unwrap();
        tokio::fs::write(&rootfs, b"").await.unwrap();

        assert!(socket.exists());
        assert!(rootfs.exists());

        mgr.cleanup_all_stale_resources().await;

        assert!(!socket.exists(), "Socket should have been removed");
        assert!(!rootfs.exists(), "Rootfs should have been removed");

        if let Err(e) = tokio::fs::remove_dir_all(&temp_dir).await {
            tracing::error!("Failed to clean up test directory {:?}: {}", temp_dir, e);
        }
    }

    #[tokio::test]
    async fn test_cleanup_does_not_touch_other_agents() {
        let temp_uuid = uuid::Uuid::new_v4();
        let temp_dir = std::env::temp_dir().join(format!("mikrom-test-gc-others-{}", temp_uuid));
        let mut cfg = FirecrackerConfig::stub();
        cfg.data_dir = temp_dir.to_string_lossy().to_string();

        let mgr = FirecrackerManager::with_config(cfg);
        let data_dir = std::path::Path::new(&mgr.fc_config.data_dir);

        let other_socket = data_dir.join("fc-other-agent-vm1.sock");
        tokio::fs::write(&other_socket, b"").await.unwrap();

        mgr.cleanup_all_stale_resources().await;

        assert!(
            other_socket.exists(),
            "File from other agent should NOT have been removed"
        );

        if let Err(e) = tokio::fs::remove_dir_all(&temp_dir).await {
            tracing::error!("Failed to clean up test directory {:?}: {}", temp_dir, e);
        }
    }

    #[tokio::test]
    async fn test_stop_vm_removes_logs_from_memory() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let vm_id = "vm-log-test";

        let config = VmConfig {
            vcpus: 1,
            memory_mib: 128,
            disk_mib: 1024,
            ..Default::default()
        };
        mgr.start_vm(
            vm_id.to_string(),
            "app-1".to_string(),
            "image".to_string(),
            config,
        )
        .await
        .unwrap();

        {
            let mut l = mgr.logs.write().await;
            l.entry(vm_id.to_string())
                .or_default()
                .push_back("test log".to_string());
        }

        assert!(mgr.logs.read().await.contains_key(vm_id));

        mgr.stop_vm(vm_id).await.unwrap();

        assert!(!mgr.logs.read().await.contains_key(vm_id));
    }

    #[test]
    fn test_bridge_config_logic() {
        let (name, ip) = FirecrackerManager::resolve_bridge_config(None);
        assert_eq!(name, "mikrom-br0");
        assert_eq!(ip, "10.0.0.1/8");

        let (_, ip_override) =
            FirecrackerManager::resolve_bridge_config(Some("10.0.1.1/24".to_string()));
        assert_eq!(ip_override, "10.0.1.1/24");
    }

    #[tokio::test]
    async fn test_get_all_vms_includes_metrics_and_socket_paths() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let vm_id = "path-test-vm";
        let socket_path = format!("{}/test.sock", mgr.fc_config.data_dir);
        let metrics_path = Some(format!("{}/test-metrics.json", mgr.fc_config.data_dir));

        // Setup VM info
        {
            let mut vms = mgr.vms.write().await;
            vms.insert(
                vm_id.to_string(),
                VmInfo {
                    vm_id: vm_id.to_string(),
                    app_id: "app1".into(),
                    image: "img".into(),
                    config: VmConfig::default(),
                    status: VmStatus::Running,
                    started_at: None,
                    error_message: None,
                },
            );
        }

        // Setup process info
        {
            let mut processes = mgr.processes.lock().await;
            let log_task = tokio::spawn(async {});
            let child = tokio::process::Command::new("true").spawn().unwrap();
            processes.insert(
                vm_id.to_string(),
                VmProcess {
                    vm_id: vm_id.to_string(),
                    child,
                    socket_path: socket_path.clone(),
                    metrics_path: metrics_path.clone(),
                    tap_name: None,
                    log_task,
                    chroot_dir: None,
                },
            );
        }

        let all_vms = mgr.get_all_vms().await;
        let vm = all_vms.iter().find(|v| v.vm_id == vm_id).unwrap();

        assert_eq!(vm.socket_path, Some(socket_path));
        assert_eq!(vm.metrics_path, metrics_path);
    }

    #[tokio::test]
    async fn test_agent_id_persistence() {
        let temp_uuid = uuid::Uuid::new_v4();
        let temp_dir = std::env::temp_dir().join(format!("mikrom-test-persistence-{}", temp_uuid));
        let mut cfg = FirecrackerConfig::stub();
        cfg.data_dir = temp_dir.to_string_lossy().to_string();

        // First run: should generate and save a new ID
        let id1 = {
            let mgr = FirecrackerManager::with_config(cfg.clone());
            mgr.agent_id.clone()
        };

        // Second run: should read the ID from the file
        let id2 = {
            let mgr = FirecrackerManager::with_config(cfg.clone());
            mgr.agent_id.clone()
        };

        assert_eq!(id1, id2, "Agent ID should be persistent across restarts");

        if let Err(e) = tokio::fs::remove_dir_all(&temp_dir).await {
            tracing::error!("Failed to clean up test directory {:?}: {}", temp_dir, e);
        }
    }

    #[tokio::test]
    async fn test_cleanup_stale_resources_after_restart() {
        let temp_uuid = uuid::Uuid::new_v4();
        let temp_dir = std::env::temp_dir().join(format!("mikrom-test-gc-restart-{}", temp_uuid));
        let mut cfg = FirecrackerConfig::stub();
        cfg.data_dir = temp_dir.to_string_lossy().to_string();

        let agent_id = {
            let mgr = FirecrackerManager::with_config(cfg.clone());
            mgr.agent_id.clone()
        };

        // Simulate stale files from a previous run
        let vm_id = "stale-vm";
        let socket = temp_dir.join(format!("fc-{}-{}.sock", agent_id, vm_id));
        let rootfs = temp_dir.join(format!("fc-{}-{}-rootfs.ext4", agent_id, vm_id));

        tokio::fs::write(&socket, b"").await.unwrap();
        tokio::fs::write(&rootfs, b"").await.unwrap();

        // Start a new manager instance (simulating restart)
        let mgr = FirecrackerManager::with_config(cfg.clone());
        assert_eq!(mgr.agent_id, agent_id);

        // Run GC
        mgr.cleanup_all_stale_resources().await;

        // Files should be gone because they aren't in mgr.processes
        assert!(
            !socket.exists(),
            "Stale socket should be cleaned up after restart"
        );
        assert!(
            !rootfs.exists(),
            "Stale rootfs should be cleaned up after restart"
        );

        if let Err(e) = tokio::fs::remove_dir_all(&temp_dir).await {
            tracing::error!("Failed to clean up test directory {:?}: {}", temp_dir, e);
        }
    }
}
