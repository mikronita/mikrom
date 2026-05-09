use crate::firecracker::api::{fc_patch, fc_put, wait_for_socket};
use crate::firecracker::config::{FirecrackerConfig, FirecrackerError, VmConfig, VmInfo, VmStatus};
use crate::firecracker::guard::VmStartupGuard;
use crate::firecracker::paths::VmPaths;
use crate::firecracker::process::{
    CommandExecutor, RealCommandExecutor, VmDetailedInfo, VmProcess,
};
use crate::logger::LogShipper;
use mikrom_proto::id::{AppId, VmId};

use std::collections::{HashMap, VecDeque};
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
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
    pub vms: Arc<RwLock<HashMap<VmId, VmInfo>>>,
    /// Thread-safe map of active Firecracker processes.
    pub processes: Arc<Mutex<HashMap<VmId, VmProcess>>>,
    /// Configuration for Firecracker and Jailer.
    pub fc_config: FirecrackerConfig,
    /// In-memory ring buffer of logs for each VM.
    pub logs: Arc<dashmap::DashMap<VmId, VecDeque<String>>>,
    /// Image builder for converting OCI images to Firecracker-compatible rootfs.
    pub builder: Arc<crate::builder::ImageBuilder>,
    /// Interface for executing system commands.
    pub executor: Arc<dyn CommandExecutor>,
    /// Tracks allocated IP addresses on the host bridge.
    pub allocated_ips: Arc<tokio::sync::Mutex<std::collections::HashSet<std::net::Ipv4Addr>>>,
    /// NATS client for log streaming.
    nats_client: Arc<RwLock<Option<async_nats::Client>>>,
    pub ebpf_manager: Arc<tokio::sync::Mutex<Option<crate::ebpf::EbpfManager>>>,
}

impl FirecrackerManager {
    pub async fn get_vm_info(&self, vm_id: &VmId) -> Option<VmInfo> {
        let vms = self.vms.read().await;
        vms.get(vm_id).cloned()
    }

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
                    vm_id: vm.vm_id,
                    app_id: vm.app_id,
                    status: vm.status,
                    error_message: vm.error_message.clone(),
                    pid,
                    ip_address: vm.config.ip_address.clone(),
                    metrics_path,
                    socket_path,
                    tap_ifindex: proc.and_then(|p| p.tap_ifindex),
                }
            })
            .collect()
    }

    pub async fn update_vm_firewall(
        &self,
        vm_id: &VmId,
        rules: Vec<mikrom_agent_ebpf_common::FirewallRule>,
    ) -> anyhow::Result<()> {
        let all_vms = self.get_all_vms().await;
        let vm = all_vms
            .iter()
            .find(|v| v.vm_id == *vm_id)
            .ok_or_else(|| anyhow::anyhow!("VM not found"))?;

        if let Some(ifindex) = vm.tap_ifindex {
            let mut ebpf = self.ebpf_manager.lock().await;
            if let Some(ebpf) = ebpf.as_mut() {
                ebpf.update_rules(ifindex, rules)?;
            }
        }
        Ok(())
    }

    /// Create a manager whose configuration is read from environment variables.
    #[must_use]
    pub async fn new() -> Self {
        let ebpf_manager = crate::ebpf::EbpfManager::load().await.ok();
        Self::with_config_and_ebpf(FirecrackerConfig::from_env(), ebpf_manager)
    }

    pub fn with_ebpf(ebpf: Option<crate::ebpf::EbpfManager>) -> Self {
        Self::with_config_and_ebpf(FirecrackerConfig::from_env(), ebpf)
    }

    pub fn with_config(fc_config: FirecrackerConfig) -> Self {
        Self::with_config_and_ebpf(fc_config, None)
    }

    pub fn with_config_and_ebpf(
        fc_config: FirecrackerConfig,
        ebpf: Option<crate::ebpf::EbpfManager>,
    ) -> Self {
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
            logs: Arc::new(dashmap::DashMap::new()),
            builder: Arc::new(builder),
            executor: Arc::new(RealCommandExecutor),
            allocated_ips: Arc::new(tokio::sync::Mutex::new(std::collections::HashSet::new())),
            nats_client: Arc::new(RwLock::new(Option::None)),
            ebpf_manager: Arc::new(tokio::sync::Mutex::new(ebpf)),
        }
    }

    pub async fn set_nats_client(&self, client: async_nats::Client) {
        let mut n = self.nats_client.write().await;
        *n = Some(client);
    }

    pub fn with_executor(mut self, executor: Arc<dyn CommandExecutor>) -> Self {
        self.executor = executor;
        self
    }

    pub fn start_background_tasks(&self) {
        let self_clone = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
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
                    to_remove.push((*vm_id, status));
                },
                Ok(None) => {
                    // Still running
                },
                Err(e) => {
                    tracing::error!(vm_id = %vm_id, error = %e, "Error checking Firecracker process status");
                },
            }
        }

        for (vm_id, exit_status) in to_remove {
            let restart_data = if let Some(proc) = processes.remove(&vm_id) {
                if let Err(e) = tokio::fs::remove_file(&proc.socket_path).await
                    && e.kind() != std::io::ErrorKind::NotFound
                {
                    tracing::debug!("Failed to remove socket {}: {}", proc.socket_path, e);
                }
                if let Some(tap) = &proc.tap_name {
                    self.cleanup_tap(tap).await;
                }

                let mut vms = self.vms.write().await;
                if let Some(vm) = vms.get_mut(&vm_id) {
                    tracing::info!(vm_id = %vm_id, current_status = ?vm.status, "Checking if VM needs auto-restart");
                    let eligibility = if vm.status == VmStatus::Running {
                        let exit_code = exit_status.code();
                        let signal = exit_status.signal();

                        tracing::error!(
                            vm_id = %vm_id,
                            exit_code = ?exit_code,
                            signal = ?signal,
                            "VM process exited unexpectedly, preparing for auto-restart"
                        );
                        if let Some(ip) = &vm.config.ip_address {
                            self.release_vm_ip(ip).await;
                        }
                        Some((vm_id, vm.app_id, vm.image.clone(), vm.config.clone()))
                    } else {
                        tracing::info!(vm_id = %vm_id, status = ?vm.status, "VM was not in Running state, skipping auto-restart");
                        None
                    };

                    vm.status = VmStatus::Stopped;
                    eligibility
                } else {
                    tracing::error!(vm_id = %vm_id, "VM not found in memory during GC cleanup");
                    None
                }
            } else {
                None
            };

            // Trigger restart outside of the processes lock
            if let Some((vid, aid, img, cfg)) = restart_data {
                let self_clone = self.clone();
                tokio::spawn(async move {
                    tracing::info!(vm_id = %vid, "Executing auto-restart after unexpected exit");
                    if let Err(e) = self_clone.start_vm(vid, aid, img, cfg).await {
                        tracing::error!(error = %e, "Auto-restart failed");
                    }
                });
            }
        }

        // Drop the lock before calling cleanup_all_stale_resources to prevent deadlock
        drop(processes);

        // 2. Clean up stale resources in /tmp (agent-specific)
        self.cleanup_all_stale_resources().await;
    }

    pub async fn start_vm(
        &self,
        vm_id: VmId,
        app_id: AppId,
        image: String,
        config: VmConfig,
    ) -> Result<(), FirecrackerError> {
        // 1. Pre-check and initial state registration
        {
            let mut vms = self.vms.write().await;
            if let Some(vm) = vms.get_mut(&vm_id) {
                if vm.status == VmStatus::Running
                    || vm.status == VmStatus::Starting
                    || vm.status == VmStatus::Stopping
                {
                    return Err(FirecrackerError::StartFailed(
                        "VM already exists and is running, starting, or stopping".to_string(),
                    ));
                }

                let old_status = vm.status;
                // Transition existing VM back to Starting
                vm.status = VmStatus::Starting;
                vm.error_message = None;
                tracing::info!(
                    vm_id = %vm_id,
                    previous_status = ?old_status,
                    "Restarting existing VM"
                );
            } else {
                vms.insert(
                    vm_id,
                    VmInfo {
                        vm_id,
                        app_id,
                        image: image.clone(),
                        config: config.clone(),
                        status: VmStatus::Starting,
                        started_at: None,
                        error_message: None,
                    },
                );
            }
        }

        // 2. Add initial log entry
        {
            self.logs
                .entry(vm_id)
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
        let vm_id_clone = vm_id;
        let app_id_clone = app_id;
        let image_clone = image.clone();
        let config_clone = config.clone();

        tracing::info!(vm_id = %vm_id, "Spawning background startup task");
        tokio::spawn(async move {
            let vid = vm_id_clone;
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
        vm_id: VmId,
        app_id: AppId,
        image: String,
        mut config: VmConfig,
    ) -> Result<(), FirecrackerError> {
        tracing::info!(vm_id = %vm_id, "Background VM startup initiated");

        let paths = VmPaths::new(&self.fc_config.data_dir, &self.agent_id, vm_id);

        // 1. Exclusivity check (Disabled for Zero-Downtime deployments)
        // self.ensure_app_exclusivity(&app_id, &vm_id).await;

        // 2. Kernel check (Stub mode check)
        let Some(kernel_path) = self.fc_config.kernel_path.clone() else {
            tracing::info!(vm_id = %vm_id, "Stub mode: marking as running");
            let mut vms = self.vms.write().await;
            if let Some(vm) = vms.get_mut(&vm_id) {
                vm.status = VmStatus::Running;
                vm.started_at = Some(chrono::Utc::now().timestamp());
            }
            return Ok(());
        };

        // 3. Prepare RootFS
        let rootfs_path = paths.rootfs_path();
        self.prepare_rootfs(
            &vm_id,
            &image,
            &rootfs_path.to_string_lossy(),
            config.port,
            config.ipv6_address.clone(),
            config.ipv6_gateway.clone(),
        )
        .await?;

        // 4. Resolve Networking
        if config.ip_address.as_deref().unwrap_or("").is_empty() {
            if let Some((ip, gw, mac)) = self.allocate_vm_network().await {
                tracing::info!(vm_id = %vm_id, ip = %ip, "Allocated IP from agent bridge subnet");
                config.ip_address = Some(ip);
                config.gateway = Some(gw);
                config.mac_address = Some(mac);
            } else {
                tracing::warn!(vm_id = %vm_id, "No available IPs in bridge subnet, starting without networking");
            }
        }

        // 4. Set up TAP if network is requested
        let (tap_name, tap_ifindex) = if config.ip_address.is_some() {
            let (tap, ifindex) = self.setup_tap(&vm_id).await?;
            // Attach eBPF filter to TAP
            let mut ebpf = self.ebpf_manager.lock().await;
            if let Some(ebpf) = ebpf.as_mut()
                && let Err(e) = ebpf.attach_tc(&tap)
            {
                tracing::warn!("Failed to attach eBPF filter to {}: {}", tap, e);
            }
            (Some(tap), Some(ifindex))
        } else {
            (None, None)
        };

        // 5. Jailer or Direct Spawn
        let (exec_binary, exec_args, active_socket_path, chroot_dir) = if self.fc_config.use_jailer
        {
            let (bin, args, host_socket, chroot) = self
                .setup_jailer(&vm_id, &kernel_path, &rootfs_path.to_string_lossy())
                .await?;

            // Cleanup stale socket on host for jailer
            if let Err(e) = tokio::fs::remove_file(&host_socket).await
                && e.kind() != std::io::ErrorKind::NotFound
            {
                tracing::debug!(
                    "Failed to remove stale jailer socket {}: {}",
                    host_socket,
                    e
                );
            }

            (bin, args, host_socket, chroot)
        } else {
            let socket_path = paths.socket_path();
            if let Err(e) = tokio::fs::remove_file(&socket_path).await
                && e.kind() != std::io::ErrorKind::NotFound
            {
                tracing::debug!(
                    "Failed to remove stale socket {}: {}",
                    socket_path.display(),
                    e
                );
            }
            (
                self.fc_config.binary.clone(),
                vec![
                    "--api-sock".to_string(),
                    socket_path.to_string_lossy().to_string(),
                ],
                socket_path.to_string_lossy().to_string(),
                None,
            )
        };

        // 6. Initialize Startup Guard
        let mut guard = VmStartupGuard::new(vm_id, PathBuf::from(&active_socket_path));
        guard.tap_name = tap_name.clone();
        guard.tap_ifindex = tap_ifindex;
        guard.chroot_dir = chroot_dir.clone().map(PathBuf::from);

        // 7. Spawn Firecracker
        let mut child = tokio::process::Command::new(&exec_binary)
            .args(&exec_args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                let msg =
                    format!("Failed to spawn firecracker process (binary: {exec_binary}): {e}");
                tracing::error!("{}", msg);
                FirecrackerError::ProcessError(msg)
            })?;

        // 8. Capture logs
        guard.log_task = Some(self.spawn_log_task(&vm_id, &app_id, &mut child).await);
        guard.child = Some(child);

        // 9. Wait for socket
        let wait_timeout = if chroot_dir.is_some() {
            Duration::from_secs(10)
        } else {
            Duration::from_secs(5)
        };

        wait_for_socket(&active_socket_path, wait_timeout).await?;

        // 10. Configure Metrics
        let metrics_host_path = self
            .setup_metrics(&vm_id, &chroot_dir, &active_socket_path, &paths)
            .await?;
        guard.metrics_path = Some(metrics_host_path.clone());

        // 11. Snapshot Restoration
        if self
            .try_restore_snapshot(&vm_id, &chroot_dir, &active_socket_path, &paths)
            .await?
        {
            self.finalize_startup(guard).await?;
            return Ok(());
        }

        // 12. Standard Boot API Calls
        self.configure_vm_api(
            &config,
            &kernel_path,
            &rootfs_path,
            &chroot_dir,
            &active_socket_path,
            tap_name.as_deref(),
        )
        .await?;

        // 13. Finalize
        self.finalize_startup(guard).await?;
        Ok(())
    }

    #[allow(dead_code)]
    async fn ensure_app_exclusivity(&self, app_id: &AppId, current_vm_id: &VmId) {
        let other_vms: Vec<VmId> = {
            let vms = self.vms.read().await;
            vms.values()
                .filter(|v| {
                    &v.app_id == app_id
                        && &v.vm_id != current_vm_id
                        && v.status != VmStatus::Stopped
                })
                .map(|v| v.vm_id)
                .collect()
        };

        for other_id in other_vms {
            tracing::info!(
                new_vm_id = %current_vm_id,
                old_vm_id = %other_id,
                app_id = %app_id,
                "Stopping existing VM for the same application to ensure exclusivity"
            );
            let _ = self.stop_vm(&other_id).await;
        }
    }

    async fn spawn_log_task(
        &self,
        vm_id: &VmId,
        app_id: &AppId,
        child: &mut tokio::process::Child,
    ) -> tokio::task::JoinHandle<()> {
        let stdout = child.stdout.take().expect("Failed to take stdout");
        let stderr = child.stderr.take().expect("Failed to take stderr");

        let shipper = LogShipper::new(
            *vm_id,
            *app_id,
            self.nats_client.read().await.clone(),
            self.logs.clone(),
        );

        shipper.spawn(stdout, stderr).await
    }

    async fn setup_metrics(
        &self,
        vm_id: &VmId,
        chroot_dir: &Option<String>,
        active_socket_path: &str,
        paths: &VmPaths,
    ) -> Result<String, FirecrackerError> {
        let (host_path, api_path) = if let Some(chroot) = chroot_dir {
            let h_path = format!("{chroot}/root/metrics.json");
            tokio::fs::write(&h_path, b"").await.map_err(|e| {
                FirecrackerError::ProcessError(format!("Failed to create metrics file: {e}"))
            })?;
            self.recursive_chown(
                &h_path,
                self.fc_config.jailer_uid,
                self.fc_config.jailer_gid,
            )
            .await?;
            (h_path, "/metrics.json".to_string())
        } else {
            let h_path = paths.metrics_path().to_string_lossy().to_string();
            (h_path.clone(), h_path)
        };

        let metrics_config = serde_json::json!({ "metrics_path": api_path }).to_string();
        if let Err(e) = fc_put(active_socket_path, "/metrics", &metrics_config).await {
            tracing::warn!(vm_id = %vm_id, "Failed to configure metrics: {e}");
        }
        Ok(host_path)
    }

    async fn try_restore_snapshot(
        &self,
        vm_id: &VmId,
        chroot_dir: &Option<String>,
        active_socket_path: &str,
        paths: &VmPaths,
    ) -> Result<bool, FirecrackerError> {
        let snapshot_path = paths.snapshot_file();
        let mem_path = paths.memory_file();

        if tokio::fs::metadata(&snapshot_path).await.is_err()
            || tokio::fs::metadata(&mem_path).await.is_err()
        {
            return Ok(false);
        }

        tracing::info!(vm_id = %vm_id, "Found snapshot, restoring VM...");

        let (load_snap, load_mem) = if let Some(chroot) = chroot_dir {
            let c_snap = format!("{chroot}/root/vm.snapshot");
            let c_mem = format!("{chroot}/root/vm.mem");
            self.ensure_file_at(&snapshot_path.to_string_lossy(), &c_snap)
                .await?;
            self.ensure_file_at(&mem_path.to_string_lossy(), &c_mem)
                .await?;
            self.recursive_chown(
                &c_snap,
                self.fc_config.jailer_uid,
                self.fc_config.jailer_gid,
            )
            .await?;
            self.recursive_chown(&c_mem, self.fc_config.jailer_uid, self.fc_config.jailer_gid)
                .await?;
            ("/vm.snapshot".to_string(), "/vm.mem".to_string())
        } else {
            (
                snapshot_path.to_string_lossy().to_string(),
                mem_path.to_string_lossy().to_string(),
            )
        };

        let body = serde_json::json!({
            "snapshot_path": load_snap,
            "mem_file_path": load_mem,
            "resume": true
        })
        .to_string();

        if let Err(e) = fc_put(active_socket_path, "/snapshot/load", &body).await {
            tracing::error!(vm_id = %vm_id, "Failed to load snapshot: {}. Falling back to normal boot.", e);
            Ok(false)
        } else {
            Ok(true)
        }
    }

    async fn configure_vm_api(
        &self,
        config: &VmConfig,
        kernel_path: &str,
        rootfs_path: &std::path::Path,
        chroot_dir: &Option<String>,
        active_socket_path: &str,
        tap_name: Option<&str>,
    ) -> Result<(), FirecrackerError> {
        let socket = active_socket_path;

        // 1. Machine Config
        let machine_config = serde_json::json!({
            "vcpu_count": config.vcpus,
            "mem_size_mib": config.memory_mib,
            "smt": false,
            "track_dirty_pages": false
        })
        .to_string();
        fc_put(socket, "/machine-config", &machine_config).await?;

        // 2. Boot Source
        let mut boot_args =
            "console=ttyS0 reboot=k panic=1 pci=off nomodules rw root=/dev/vda init=/mikrom-init"
                .to_string();
        if let (Some(ip), Some(gw)) = (&config.ip_address, &config.gateway) {
            let mask = config.netmask.as_deref().unwrap_or("255.255.255.0");
            boot_args.push_str(&format!(" ip={ip}::{gw}:{mask}::eth0:off"));
        }

        if let (Some(ipv6), Some(gw6)) = (&config.ipv6_address, &config.ipv6_gateway) {
            // Kernel ip= format for IPv6: ip=[addr]::[gw]:prefix::device:off
            boot_args.push_str(&format!(" ip=[{ipv6}]::[{gw6}]:64::eth0:off"));
        }

        let kernel_api_path = if chroot_dir.is_some() {
            "/vmlinux.bin".to_string()
        } else {
            kernel_path.to_string()
        };
        let boot_source =
            serde_json::json!({ "kernel_image_path": kernel_api_path, "boot_args": boot_args })
                .to_string();
        fc_put(socket, "/boot-source", &boot_source).await?;

        // 3. Root Drive
        let rootfs_api_path = if chroot_dir.is_some() {
            "/rootfs.ext4".to_string()
        } else {
            rootfs_path.to_string_lossy().to_string()
        };
        let drive_json = serde_json::json!({ "drive_id": "rootfs", "path_on_host": rootfs_api_path, "is_root_device": true, "is_read_only": false }).to_string();
        fc_put(socket, "/drives/rootfs", &drive_json).await?;

        // 4. Network
        if let Some(tap) = tap_name {
            let net_json = serde_json::json!({
                "iface_id": "eth0",
                "guest_mac": config.mac_address.as_deref().unwrap_or("AA:BB:CC:DD:EE:01"),
                "host_dev_name": tap
            })
            .to_string();
            fc_put(socket, "/network-interfaces/eth0", &net_json).await?;
        }

        // 5. Additional Volumes
        for vol in &config.volumes {
            let vol_host_path = self.ensure_volume(&vol.volume_id, vol.size_mib).await?;
            let vol_api_path = if let Some(chroot) = chroot_dir {
                let filename = format!("{}.ext4", vol.volume_id);
                let c_path = format!("{chroot}/root/{filename}");
                self.ensure_file_at(&vol_host_path, &c_path).await?;
                self.recursive_chown(
                    &c_path,
                    self.fc_config.jailer_uid,
                    self.fc_config.jailer_gid,
                )
                .await?;
                format!("/{filename}")
            } else {
                vol_host_path
            };

            let vol_json = serde_json::json!({
                "drive_id": vol.volume_id,
                "path_on_host": vol_api_path,
                "is_root_device": false,
                "is_read_only": vol.read_only
            })
            .to_string();
            fc_put(socket, &format!("/drives/{}", vol.volume_id), &vol_json).await?;
        }

        // 6. Start Instance
        fc_put(
            socket,
            "/actions",
            &serde_json::json!({ "action_type": "InstanceStart" }).to_string(),
        )
        .await?;

        // 7. Add host route for IPv6
        if let Some(ipv6) = &config.ipv6_address {
            let _ = tokio::process::Command::new("ip")
                .args(["-6", "route", "add", ipv6, "dev", "mikrom-br0"])
                .output()
                .await;
        }

        Ok(())
    }

    async fn finalize_startup(&self, guard: VmStartupGuard) -> Result<(), FirecrackerError> {
        let vm_id = guard.vm_id;
        let vm_process = guard.commit();

        {
            let mut vms = self.vms.write().await;
            if let Some(vm) = vms.get_mut(&vm_id) {
                vm.status = VmStatus::Running;
                vm.started_at = Some(chrono::Utc::now().timestamp());
            }
        }

        self.processes.lock().await.insert(vm_id, vm_process);
        tracing::info!(vm_id = %vm_id, "Firecracker VM successfully started");
        Ok(())
    }

    async fn prepare_rootfs(
        &self,
        vm_id: &VmId,
        image: &str,
        rootfs_path: &str,
        port: u32,
        ipv6_addr: Option<String>,
        ipv6_gw: Option<String>,
    ) -> Result<(), FirecrackerError> {
        tracing::info!(vm_id = %vm_id, rootfs_path = %rootfs_path, "Preparing rootfs");

        let dst_path = std::path::Path::new(rootfs_path);
        if dst_path.exists() {
            tracing::info!(vm_id = %vm_id, rootfs_path = %rootfs_path, "Rootfs already exists, skipping preparation");
            return Ok(());
        }

        let image_path = std::path::Path::new(image);
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
                .docker_to_ext4(image, dst_path, port, ipv6_addr, ipv6_gw)
                .await
                .map_err(|e| {
                    FirecrackerError::ProcessError(format!("Image builder failed: {e}"))
                })?;
        }
        Ok(())
    }

    pub async fn restart_vm(&self, vm_id: &VmId) -> Result<(), FirecrackerError> {
        let (app_id, image, config) = {
            let vms = self.vms.read().await;
            let vm = vms
                .get(vm_id)
                .ok_or_else(|| FirecrackerError::VmNotFound(vm_id.to_string()))?;
            (vm.app_id, vm.image.clone(), vm.config.clone())
        };

        tracing::info!(vm_id = %vm_id, "Restarting VM...");
        let _ = self.stop_vm(vm_id).await; // Best effort stop
        self.start_vm(*vm_id, app_id, image, config).await?;
        Ok(())
    }

    #[tracing::instrument(skip(self), fields(vm_id = %vm_id))]
    pub async fn stop_vm(&self, vm_id: &VmId) -> Result<(), FirecrackerError> {
        {
            let mut vms = self.vms.write().await;
            match vms.get_mut(vm_id) {
                Some(vm) => vm.status = VmStatus::Stopping,
                None => return Err(FirecrackerError::VmNotFound(vm_id.to_string())),
            }
        }

        self.logs.remove(vm_id);

        if let Some(mut proc) = self.processes.lock().await.remove(vm_id) {
            proc.log_task.abort();

            tracing::info!(vm_id = %vm_id, "Sending kill signal to Firecracker process for stopping");
            if let Err(e) = proc.child.kill().await {
                tracing::error!(vm_id = %vm_id, "Failed to send kill signal to Firecracker: {}", e);
            }
            let _ = proc.child.wait().await;
            tracing::info!(vm_id = %vm_id, "Firecracker process terminated");
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

            if let Some(ref tap) = proc.tap_name {
                self.cleanup_tap(tap).await;
            }
        }

        let mut vms = self.vms.write().await;
        if let Some(vm) = vms.get_mut(vm_id) {
            if let Some(ip) = &vm.config.ip_address {
                self.release_vm_ip(ip).await;
            }
            vm.status = VmStatus::Stopped;
        }

        Ok(())
    }

    /// Completely purge all resources associated with a VM ID.
    /// This includes stopping it if it's running, and deleting all disk artifacts.
    #[tracing::instrument(skip(self), fields(vm_id = %vm_id))]
    pub async fn delete_vm(&self, vm_id: &VmId) -> Result<(), FirecrackerError> {
        tracing::info!("Purging all resources for VM");

        // 1. Stop the VM if it's running
        let _ = self.stop_vm(vm_id).await;

        // 2. Remove VM info from memory
        {
            let mut vms = self.vms.write().await;
            vms.remove(vm_id);
        }

        // 3. Forced cleanup of file artifacts (in case they weren't in processes map)

        // Cleanup rootfs
        let rootfs_path = std::path::Path::new(&self.fc_config.data_dir)
            .join(format!("fc-{}-{}-rootfs.ext4", self.agent_id, vm_id));
        let _ = tokio::fs::remove_file(&rootfs_path).await;

        // Cleanup snapshots
        let snapshot_dir = std::path::Path::new(&self.fc_config.data_dir).join("snapshots");
        let _ = tokio::fs::remove_file(snapshot_dir.join(format!("{vm_id}.snapshot"))).await;
        let _ = tokio::fs::remove_file(snapshot_dir.join(format!("{vm_id}.mem"))).await;

        // Cleanup jailer chroot (Crucial fix for user request)
        let chroot_dir = self.get_chroot_dir(vm_id);

        if chroot_dir.exists() {
            tracing::info!(chroot_dir = ?chroot_dir, "Removing jailer chroot directory");
            if let Err(e) = tokio::fs::remove_dir_all(&chroot_dir).await {
                tracing::error!("Failed to remove chroot directory {:?}: {}", chroot_dir, e);
            }
        }

        Ok(())
    }

    #[tracing::instrument(skip(self), fields(vm_id = %vm_id))]
    pub async fn pause_vm(&self, vm_id: &VmId) -> Result<(), FirecrackerError> {
        let mut processes = self.processes.lock().await;
        let proc = processes
            .get_mut(vm_id)
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
            "mem_file_path": mem_path,
            "version": self.fc_config.fc_version
        })
        .to_string();

        fc_put(&proc.socket_path, "/snapshot/create", &snapshot_body).await?;

        // After snapshotting, we terminate the process to free up resources.
        // We keep the files (rootfs, snapshot, mem) to allow resumption later.
        proc.log_task.abort();

        tracing::info!(vm_id = %vm_id, "Sending kill signal to Firecracker process for hibernation");
        if let Err(e) = proc.child.kill().await {
            tracing::error!(vm_id = %vm_id, "Failed to send kill signal to Firecracker: {}", e);
        }

        let _ = proc.child.wait().await;
        tracing::info!(vm_id = %vm_id, "Firecracker process terminated for hibernation");

        let socket_path = proc.socket_path.clone();

        // Remove from active processes
        processes.remove(vm_id);
        drop(processes);

        // Remove the socket file as it's no longer valid
        if let Err(e) = tokio::fs::remove_file(&socket_path).await
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::debug!("Failed to remove socket {}: {}", socket_path, e);
        }

        let mut vms = self.vms.write().await;
        if let Some(vm) = vms.get_mut(vm_id) {
            vm.status = VmStatus::Paused;
        }

        tracing::info!(vm_id = %vm_id, "VM paused and process terminated successfully");
        Ok(())
    }

    #[tracing::instrument(skip(self), fields(vm_id = %vm_id))]
    pub async fn resume_vm(&self, vm_id: &VmId) -> Result<(), FirecrackerError> {
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

        let config = vm_info.config.clone();

        self.start_vm(*vm_id, vm_info.app_id, vm_info.image.clone(), config)
            .await?;
        Ok(())
    }

    pub async fn get_vm_status(&self, vm_id: &VmId) -> Result<VmStatus, FirecrackerError> {
        let vms = self.vms.read().await;
        match vms.get(vm_id) {
            Some(vm) => Ok(vm.status),
            None => Err(FirecrackerError::VmNotFound(vm_id.to_string())),
        }
    }

    pub async fn list_vms(&self) -> Vec<VmInfo> {
        self.vms.read().await.values().cloned().collect()
    }

    pub async fn get_vm(&self, vm_id: &VmId) -> Option<VmInfo> {
        self.vms.read().await.get(vm_id).cloned()
    }

    pub fn get_logs(&self, vm_id: &VmId) -> Vec<String> {
        self.logs
            .get(vm_id)
            .map(|logs| logs.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub async fn get_pids(&self) -> HashMap<VmId, u32> {
        let mut pids = HashMap::new();
        let processes = self.processes.lock().await;
        for (vm_id, proc) in processes.iter() {
            if let Some(pid) = proc.child.id() {
                pids.insert(*vm_id, pid);
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

    async fn set_failed(&self, vm_id: &VmId, msg: String) {
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

        let active_vm_ids: std::collections::HashSet<VmId> = {
            let processes = self.processes.lock().await;
            let vms = self.vms.read().await;
            let mut ids: std::collections::HashSet<VmId> = processes.keys().cloned().collect();
            // Also protect VMs that are currently starting
            for id in vms.keys() {
                ids.insert(*id);
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

        // 1. Create bridge if not exists
        let output = tokio::process::Command::new("ip")
            .args(["link", "add", "name", &bridge_name, "type", "bridge"])
            .output()
            .await;

        if let Ok(out) = output
            && !out.status.success()
        {
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.contains("File exists") {
                tracing::warn!("Failed to create bridge {}: {}", bridge_name, err);
            }
        }

        // 2. Set bridge UP
        let output = tokio::process::Command::new("ip")
            .args(["link", "set", "dev", &bridge_name, "up"])
            .output()
            .await;

        if let Ok(out) = output
            && !out.status.success()
        {
            tracing::warn!(
                "Failed to set bridge {} UP: {}",
                bridge_name,
                String::from_utf8_lossy(&out.stderr)
            );
        }

        // 3. Clear any existing IPv6 addresses in the fd00:: range to avoid conflicts
        // This is crucial if the agent was previously running with a /8 mask
        let output = tokio::process::Command::new("ip")
            .args([
                "-6",
                "addr",
                "flush",
                "dev",
                &bridge_name,
                "scope",
                "global",
            ])
            .output()
            .await;

        if let Ok(out) = output
            && !out.status.success()
        {
            tracing::warn!(
                "Failed to flush IPv6 addresses on bridge {}: {}",
                bridge_name,
                String::from_utf8_lossy(&out.stderr)
            );
        }

        // 4. Add IPv4 address (ignore error if exists)
        let output = tokio::process::Command::new("ip")
            .args(["addr", "add", &bridge_ip, "dev", &bridge_name])
            .output()
            .await;

        if let Ok(out) = output
            && !out.status.success()
        {
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.contains("File exists") {
                tracing::warn!(
                    "Failed to add IPv4 address {} to bridge {}: {}",
                    bridge_ip,
                    bridge_name,
                    err
                );
            }
        }

        // 5. Assign stable IPv6 gateway addresses
        // Using /128 for the ULA address so it doesn't conflict with the WireGuard fd00::/8 route
        let output = tokio::process::Command::new("ip")
            .args(["-6", "addr", "add", "fd00::1/128", "dev", &bridge_name])
            .output()
            .await;

        if let Ok(out) = output
            && !out.status.success()
        {
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.contains("File exists") {
                tracing::warn!(
                    "Failed to add IPv6 address fd00::1/128 to bridge {}: {}",
                    bridge_name,
                    err
                );
            }
        }

        // Ensure fe80::1 is also present for link-local gatewaying
        let output = tokio::process::Command::new("ip")
            .args(["-6", "addr", "add", "fe80::1/64", "dev", &bridge_name])
            .output()
            .await;

        if let Ok(out) = output
            && !out.status.success()
        {
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.contains("File exists") {
                tracing::warn!(
                    "Failed to add IPv6 link-local address fe80::1/64 to bridge {}: {}",
                    bridge_name,
                    err
                );
            }
        }

        // 6. Enable forwarding
        tokio::process::Command::new("sysctl")
            .args(["-w", "net.ipv4.ip_forward=1"])
            .output()
            .await
            .map_err(|e| {
                FirecrackerError::ProcessError(format!("Failed to enable IPv4 forwarding: {e}"))
            })?;

        tokio::process::Command::new("sysctl")
            .args(["-w", "net.ipv6.conf.all.forwarding=1"])
            .output()
            .await
            .map_err(|e| {
                FirecrackerError::ProcessError(format!("Failed to enable IPv6 forwarding: {e}"))
            })?;

        // 7. Setup NAT
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

        if let Ok(o) = output
            && !o.status.success()
        {
            tracing::warn!(
                "Failed to setup iptables MASQUERADE: {}",
                String::from_utf8_lossy(&o.stderr)
            );
        }

        Ok(())
    }

    async fn setup_tap(&self, vm_id: &VmId) -> Result<(String, u32), FirecrackerError> {
        let tap_name = format!("m-tap-{}", &vm_id.to_string()[..8]);

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

        // Get ifindex
        let ifindex = std::fs::read_to_string(format!("/sys/class/net/{}/ifindex", tap_name))
            .map_err(|e| FirecrackerError::ProcessError(format!("Failed to read ifindex: {e}")))?
            .trim()
            .parse::<u32>()
            .map_err(|e| FirecrackerError::ProcessError(format!("Failed to parse ifindex: {e}")))?;

        Ok((tap_name, ifindex))
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

    /// Helper to get the jailer chroot directory for a VM.
    fn get_chroot_dir(&self, vm_id: &VmId) -> std::path::PathBuf {
        let exec_name = std::path::Path::new(&self.fc_config.binary)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("firecracker");

        std::path::Path::new(&self.fc_config.chroot_base)
            .join(exec_name)
            .join(vm_id.to_string())
    }

    async fn setup_jailer(
        &self,
        vm_id: &VmId,
        kernel_host_path: &str,
        rootfs_host_path: &str,
    ) -> Result<(String, Vec<String>, String, Option<String>), FirecrackerError> {
        let chroot_dir = self.get_chroot_dir(vm_id);
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

#[cfg(test)]
impl FirecrackerManager {
    pub async fn set_status_for_test(&self, vm_id: &VmId, status: VmStatus) {
        if let Some(vm) = self.vms.write().await.get_mut(vm_id) {
            vm.status = status;
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn insert_process_for_test(
        &self,
        vm_id: &VmId,
        child: tokio::process::Child,
        socket_path: String,
    ) {
        let log_task = tokio::spawn(async {});
        self.processes.lock().await.insert(
            *vm_id,
            VmProcess {
                vm_id: *vm_id,
                child,
                socket_path,
                metrics_path: None,
                tap_name: None,
                tap_ifindex: None,
                log_task,
                chroot_dir: None,
            },
        );
        let mut vms = self.vms.write().await;
        vms.insert(
            *vm_id,
            VmInfo {
                vm_id: *vm_id,
                app_id: AppId::new(),
                image: "test-image".to_string(),
                status: VmStatus::Running,
                config: VmConfig::default(),
                started_at: None,
                error_message: None,
            },
        );
    }

    #[allow(dead_code)]
    pub(crate) async fn has_process(&self, vm_id: &VmId) -> bool {
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
            health_check_path: "/".to_string(),
            ipv6_address: None,
            ipv6_gateway: None,
        }
    }

    async fn start(mgr: &FirecrackerManager, vm_id: &VmId) {
        mgr.start_vm(*vm_id, AppId::new(), "nginx:latest".to_string(), config())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_start_vm_succeeds() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        assert!(
            mgr.start_vm(VmId::new(), AppId::new(), "img.ext4".to_string(), config())
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn test_started_vm_has_starting_status() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let vm_id = VmId::new();
        start(&mgr, &vm_id).await;
        assert_eq!(mgr.get_vm_status(&vm_id).await.unwrap(), VmStatus::Starting);
    }

    #[tokio::test]
    async fn test_start_duplicate_vm_fails() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let vm_id = VmId::new();
        start(&mgr, &vm_id).await;
        let result = mgr
            .start_vm(vm_id, AppId::new(), "img.ext4".to_string(), config())
            .await;
        assert!(matches!(result, Err(FirecrackerError::StartFailed(_))));
    }

    #[tokio::test]
    async fn test_stop_vm_transitions_to_stopping() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let vm_id = VmId::new();
        start(&mgr, &vm_id).await;
        assert!(mgr.stop_vm(&vm_id).await.is_ok());
        assert_eq!(mgr.get_vm_status(&vm_id).await.unwrap(), VmStatus::Stopped);
    }

    #[tokio::test]
    async fn test_stop_nonexistent_vm_returns_error() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        assert!(matches!(
            mgr.stop_vm(&VmId::new()).await,
            Err(FirecrackerError::VmNotFound(_))
        ));
    }

    #[tokio::test]
    async fn test_get_status_nonexistent_returns_error() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        assert!(matches!(
            mgr.get_vm_status(&VmId::new()).await,
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
        start(&mgr, &VmId::new()).await;
        start(&mgr, &VmId::new()).await;
        assert_eq!(mgr.list_vms().await.len(), 2);
    }

    #[tokio::test]
    async fn test_get_vm_returns_correct_info() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let vm_id = VmId::new();
        let app_id = AppId::new();
        mgr.start_vm(vm_id, app_id, "ubuntu:24.04".to_string(), config())
            .await
            .unwrap();
        let vm = mgr.get_vm(&vm_id).await.unwrap();
        assert_eq!(vm.app_id, app_id);
        assert_eq!(vm.image, "ubuntu:24.04");
        assert_eq!(vm.config.vcpus, 1);
        assert_eq!(vm.config.memory_mib, 256);
        assert!(vm.config.volumes.is_empty());
    }

    #[tokio::test]
    async fn test_get_vm_nonexistent_returns_none() {
        assert!(
            FirecrackerManager::with_config(FirecrackerConfig::stub())
                .get_vm(&VmId::new())
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
        let vm_id = VmId::new();
        start(&mgr, &vm_id).await;
        let vm = mgr.get_vm(&vm_id).await.unwrap();
        let json = serde_json::to_string(&vm).unwrap();
        let restored: VmInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.vm_id, vm_id);
        assert_eq!(restored.status, VmStatus::Starting);
    }

    #[tokio::test]
    async fn test_error_messages_contain_vm_id() {
        let err = FirecrackerError::VmNotFound("vm-99".to_string());
        assert!(err.to_string().contains("vm-99"));
    }

    #[tokio::test]
    async fn test_concurrent_start_different_vms() {
        use std::sync::Arc;
        let mgr = Arc::new(FirecrackerManager::with_config(FirecrackerConfig::stub()));
        let mut handles = vec![];

        for _ in 0..10 {
            let m = mgr.clone();
            handles.push(tokio::spawn(async move {
                let result = m
                    .start_vm(
                        VmId::new(),
                        AppId::new(),
                        "nginx:latest".to_string(),
                        config(),
                    )
                    .await;
                assert!(result.is_ok());
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        assert_eq!(mgr.list_vms().await.len(), 10);
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
        let vm_id = VmId::new();
        let app_id = AppId::new();

        for _ in 0..10 {
            let m = mgr.clone();
            let counter = success_count.clone();
            let vid = vm_id;
            let aid = app_id;
            handles.push(tokio::spawn(async move {
                if m.start_vm(vid, aid, "nginx".to_string(), config())
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
    }

    #[tokio::test]
    async fn test_wait_for_socket_times_out_when_file_never_appears() {
        let result =
            wait_for_socket("/tmp/fc-nonexistent-socket.sock", Duration::from_millis(50)).await;
        assert!(matches!(result, Err(FirecrackerError::SocketTimeout(_))));
    }

    #[tokio::test]
    async fn test_delete_vm_purges_all_resources() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let vm_id = VmId::new();

        // 1. Setup fake files
        let data_dir = std::path::Path::new(&mgr.fc_config.data_dir);
        tokio::fs::create_dir_all(data_dir).await.unwrap();

        let rootfs = data_dir.join(format!("fc-{}-{}-rootfs.ext4", mgr.agent_id, vm_id));
        tokio::fs::write(&rootfs, b"fake").await.unwrap();

        // 2. Register VM in memory
        mgr.set_status_for_test(&vm_id, VmStatus::Stopped).await;

        // 3. Perform delete
        mgr.delete_vm(&vm_id).await.expect("delete_vm failed");

        // 4. Verify everything is gone
        assert!(!rootfs.exists());
    }
}
