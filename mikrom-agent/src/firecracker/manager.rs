use crate::ceph::StorageProvider;
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
use std::ffi::CString;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::sync::{Mutex, RwLock};

use futures::stream::TryStreamExt;
use netlink_packet_route::route::{RouteAddress, RouteAttribute};
use std::net::IpAddr;

const TUNSETIFF: libc::c_ulong = 0x400454ca;
const TUNSETPERSIST: libc::c_ulong = 0x400454cb;
const TUNSETOWNER: libc::c_ulong = 0x400454cc;

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

struct StartupContext {
    exec_binary: String,
    exec_args: Vec<String>,
    active_socket_path: String,
    chroot_dir: Option<String>,
}

impl FirecrackerManager {
    fn ipv6_route_prefix(ipv6: &str) -> Option<String> {
        let addr: std::net::Ipv6Addr = ipv6.parse().ok()?;
        let seg = addr.segments();
        let prefix = std::net::Ipv6Addr::new(seg[0], seg[1], seg[2], seg[3], 0, 0, 0, 0);
        Some(format!("{prefix}/64"))
    }

    fn snapshot_create_body(snapshot_path: &str, mem_path: &str) -> String {
        serde_json::json!({
            "snapshot_type": "Full",
            "snapshot_path": snapshot_path,
            "mem_file_path": mem_path,
        })
        .to_string()
    }

    fn snapshot_paths(
        &self,
        vm_id: &VmId,
        chroot_dir: Option<&str>,
    ) -> (
        std::path::PathBuf,
        std::path::PathBuf,
        std::path::PathBuf,
        std::path::PathBuf,
    ) {
        let snapshot_dir = std::path::Path::new(&self.fc_config.data_dir).join("snapshots");
        let host_snapshot_path = snapshot_dir.join(format!("{vm_id}.snapshot"));
        let host_mem_path = snapshot_dir.join(format!("{vm_id}.mem"));

        match chroot_dir {
            Some(_) => (
                host_snapshot_path,
                host_mem_path,
                std::path::PathBuf::from("/vm.snapshot"),
                std::path::PathBuf::from("/vm.mem"),
            ),
            None => (
                host_snapshot_path.clone(),
                host_mem_path.clone(),
                host_snapshot_path,
                host_mem_path,
            ),
        }
    }

    async fn rtnl_handle(&self) -> Result<rtnetlink::Handle, FirecrackerError> {
        let (connection, handle, _) = rtnetlink::new_connection().map_err(|e| {
            FirecrackerError::ProcessError(format!("Failed to create netlink connection: {e}"))
        })?;
        tokio::spawn(connection);
        Ok(handle)
    }

    async fn get_link_index(
        &self,
        handle: &rtnetlink::Handle,
        name: &str,
    ) -> Result<Option<u32>, FirecrackerError> {
        let mut links = handle.link().get().match_name(name.to_string()).execute();
        match links.try_next().await {
            Ok(Some(msg)) => Ok(Some(msg.header.index)),
            Ok(None) => Ok(None),
            Err(e) => Err(FirecrackerError::ProcessError(format!(
                "Failed to get link index for {name}: {e}"
            ))),
        }
    }

    async fn set_link_up(
        &self,
        handle: &rtnetlink::Handle,
        index: u32,
    ) -> Result<(), FirecrackerError> {
        handle.link().set(index).up().execute().await.map_err(|e| {
            FirecrackerError::ProcessError(format!("Failed to set link {index} up: {e}"))
        })
    }

    async fn set_link_mtu(
        &self,
        handle: &rtnetlink::Handle,
        index: u32,
        mtu: u32,
    ) -> Result<(), FirecrackerError> {
        handle
            .link()
            .set(index)
            .mtu(mtu)
            .execute()
            .await
            .map_err(|e| {
                FirecrackerError::ProcessError(format!(
                    "Failed to set link {index} MTU to {mtu}: {e}"
                ))
            })
    }

    async fn add_ip_address(
        &self,
        handle: &rtnetlink::Handle,
        index: u32,
        address: IpAddr,
        prefix_len: u8,
    ) -> Result<(), FirecrackerError> {
        match handle
            .address()
            .add(index, address, prefix_len)
            .execute()
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("File exists") || err_str.contains("os error 17") {
                    Ok(())
                } else {
                    Err(FirecrackerError::ProcessError(format!(
                        "Failed to add address {address}/{prefix_len} to link {index}: {e}"
                    )))
                }
            },
        }
    }

    fn parse_ip_cidr(&self, cidr: &str) -> Result<(IpAddr, u8), FirecrackerError> {
        if let Some((ip_str, prefix_str)) = cidr.split_once('/') {
            let ip: IpAddr = ip_str.parse().map_err(|e| {
                FirecrackerError::ProcessError(format!("Failed to parse IP address {ip_str}: {e}"))
            })?;
            let prefix: u8 = prefix_str.parse().map_err(|e| {
                FirecrackerError::ProcessError(format!("Failed to parse prefix {prefix_str}: {e}"))
            })?;
            Ok((ip, prefix))
        } else {
            let ip: IpAddr = cidr.parse().map_err(|e| {
                FirecrackerError::ProcessError(format!("Failed to parse IP address {cidr}: {e}"))
            })?;
            let prefix = if ip.is_ipv6() { 128 } else { 32 };
            Ok((ip, prefix))
        }
    }

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
                let tap_name = proc.and_then(|p| p.tap_name.clone());

                VmDetailedInfo {
                    vm_id: vm.vm_id,
                    app_id: vm.app_id,
                    status: vm.status,
                    error_message: vm.error_message.clone(),
                    pid,
                    metrics_path,
                    socket_path,
                    tap_name,
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
            let restart_data = self
                .handle_gc_process_exit(&vm_id, exit_status, &mut processes)
                .await;

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

    async fn handle_gc_process_exit(
        &self,
        vm_id: &VmId,
        exit_status: std::process::ExitStatus,
        processes: &mut HashMap<VmId, VmProcess>,
    ) -> Option<(VmId, AppId, String, VmConfig)> {
        let proc = processes.remove(vm_id)?;
        self.cleanup_exited_process_artifacts(vm_id, &proc).await;

        let mut vms = self.vms.write().await;
        let vm = vms.get_mut(vm_id)?;
        tracing::info!(
            vm_id = %vm_id,
            current_status = ?vm.status,
            "Checking if VM needs auto-restart"
        );

        let restart_data = if vm.status == VmStatus::Running {
            tracing::error!(
                vm_id = %vm_id,
                exit_code = ?exit_status.code(),
                signal = ?exit_status.signal(),
                "VM process exited unexpectedly, preparing for auto-restart"
            );
            if let Some(ip) = &vm.config.ip_address {
                self.release_vm_ip(ip).await;
            }
            Some((*vm_id, vm.app_id, vm.image.clone(), vm.config.clone()))
        } else {
            tracing::info!(
                vm_id = %vm_id,
                status = ?vm.status,
                "VM was not in Running state, skipping auto-restart"
            );
            None
        };

        vm.status = VmStatus::Stopped;
        restart_data
    }

    async fn cleanup_exited_process_artifacts(&self, vm_id: &VmId, proc: &VmProcess) {
        self.cleanup_process_paths(vm_id, proc).await;
        self.cleanup_process_chroot(vm_id, proc).await;
        self.cleanup_process_volumes(vm_id).await;
    }

    async fn cleanup_process_paths(&self, vm_id: &VmId, proc: &VmProcess) {
        if let Err(e) = tokio::fs::remove_file(&proc.socket_path).await
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::debug!("Failed to remove socket {}: {}", proc.socket_path, e);
        }

        let paths = crate::firecracker::paths::VmPaths::new(
            &self.fc_config.data_dir,
            &self.agent_id,
            *vm_id,
        );
        let rootfs_path = paths.rootfs_path();
        if let Err(e) = tokio::fs::remove_file(&rootfs_path).await
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::debug!("Failed to remove rootfs {:?}: {}", rootfs_path, e);
        }

        let snap_path = paths.snapshot_file();
        let mem_path = paths.memory_file();

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
    }

    async fn cleanup_process_chroot(&self, vm_id: &VmId, proc: &VmProcess) {
        if let Some(chroot) = &proc.chroot_dir {
            tracing::info!(vm_id = %vm_id, chroot_dir = %chroot, "Cleaning up jailer chroot");
            if let Err(e) = tokio::fs::remove_dir_all(chroot).await {
                tracing::error!("Failed to remove chroot directory {}: {}", chroot, e);
            }
        }
    }

    async fn cleanup_process_volumes(&self, vm_id: &VmId) {
        let volumes = {
            let vms = self.vms.read().await;
            vms.get(vm_id)
                .map(|vm| vm.config.volumes.clone())
                .unwrap_or_default()
        };

        let storage = crate::ceph::CephRbd;
        for vol in volumes {
            if !vol.pool_name.is_empty() {
                let dev_path = format!("/dev/rbd/{}/{}", vol.pool_name, vol.volume_id);
                if let Err(e) = storage.unmap_volume(&dev_path).await {
                    tracing::warn!("Failed to unmap volume {}: {}", dev_path, e);
                }
            }
        }
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

        if let Some(kernel_path) = &self.fc_config.kernel_path {
            self.validate_kernel_image(kernel_path).await?;
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

        // 2. Kernel check (Stub mode check)
        let Some(kernel_path) = self.fc_config.kernel_path.clone() else {
            self.mark_vm_running(&vm_id).await;
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

        let (tap_name, tap_ifindex) = self.configure_vm_networking(&vm_id, &mut config).await?;
        let startup = self
            .resolve_startup_context(&vm_id, &kernel_path, &rootfs_path, &paths)
            .await?;

        let mut guard = self.build_startup_guard(
            vm_id,
            &startup.active_socket_path,
            tap_name.clone(),
            tap_ifindex,
            startup.chroot_dir.clone(),
        );

        let mut child = self.spawn_firecracker_process(&startup).await?;
        guard.log_task = Some(
            self.spawn_log_task(
                &vm_id,
                &app_id,
                &mut child,
                guard.app_started.clone(),
                guard.app_started_at_ms.clone(),
            )
            .await,
        );
        guard.child = Some(child);

        self.wait_for_firecracker_socket(&startup).await?;
        guard.metrics_path = Some(
            self.setup_metrics(
                &vm_id,
                &startup.chroot_dir,
                &startup.active_socket_path,
                &paths,
            )
            .await?,
        );

        if self
            .try_restore_snapshot(
                &vm_id,
                &startup.chroot_dir,
                &startup.active_socket_path,
                &paths,
            )
            .await?
        {
            self.mark_vm_app_started_now(&mut guard);
            self.finalize_startup(guard).await?;
            return Ok(());
        }

        self.configure_vm_api(
            &config,
            &kernel_path,
            &rootfs_path,
            &startup.chroot_dir,
            &startup.active_socket_path,
            tap_name.as_deref(),
        )
        .await?;

        self.finalize_startup(guard).await?;
        Ok(())
    }

    async fn mark_vm_running(&self, vm_id: &VmId) {
        tracing::info!(vm_id = %vm_id, "Stub mode: marking as running");
        let mut vms = self.vms.write().await;
        if let Some(vm) = vms.get_mut(vm_id) {
            vm.status = VmStatus::Running;
            vm.started_at = Some(chrono::Utc::now().timestamp());
        }
    }

    async fn configure_vm_networking(
        &self,
        vm_id: &VmId,
        config: &mut VmConfig,
    ) -> Result<(Option<String>, Option<u32>), FirecrackerError> {
        if config.ip_address.as_deref().unwrap_or("").is_empty() {
            if let Some((ip, gw, mac)) = self.allocate_vm_network().await {
                tracing::info!(vm_id = %vm_id, ip = %ip, "Allocated IP from agent bridge subnet");
                config.ip_address = Some(ip);
                config.gateway = Some(gw);
                config.mac_address = Some(mac);
            } else {
                tracing::warn!(
                    vm_id = %vm_id,
                    "No available IPs in bridge subnet, starting without networking"
                );
            }
        }

        if config.ip_address.is_some() {
            let (tap, ifindex) = self.setup_tap(vm_id).await?;
            self.attach_tc_best_effort(&tap).await;
            Ok((Some(tap), Some(ifindex)))
        } else {
            Ok((None, None))
        }
    }

    async fn attach_tc_best_effort(&self, tap: &str) {
        let mut ebpf = self.ebpf_manager.lock().await;
        if let Some(ebpf) = ebpf.as_mut()
            && let Err(e) = ebpf.attach_tc(tap)
        {
            tracing::warn!("Failed to attach eBPF filter to {}: {}", tap, e);
        }
    }

    async fn resolve_startup_context(
        &self,
        vm_id: &VmId,
        kernel_path: &str,
        rootfs_path: &std::path::Path,
        paths: &VmPaths,
    ) -> Result<StartupContext, FirecrackerError> {
        if self.fc_config.use_jailer {
            let (bin, args, host_socket, chroot) = self
                .setup_jailer(vm_id, kernel_path, &rootfs_path.to_string_lossy())
                .await?;

            self.remove_stale_socket(&host_socket).await;
            Ok(StartupContext {
                exec_binary: bin,
                exec_args: args,
                active_socket_path: host_socket,
                chroot_dir: chroot,
            })
        } else {
            let socket_path = paths.socket_path();
            self.remove_stale_socket(&socket_path).await;
            Ok(StartupContext {
                exec_binary: self.fc_config.binary.clone(),
                exec_args: vec![
                    "--api-sock".to_string(),
                    socket_path.to_string_lossy().to_string(),
                ],
                active_socket_path: socket_path.to_string_lossy().to_string(),
                chroot_dir: None,
            })
        }
    }

    async fn remove_stale_socket<P: AsRef<std::path::Path>>(&self, socket_path: P) {
        if let Err(e) = tokio::fs::remove_file(socket_path.as_ref()).await
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::debug!(
                "Failed to remove stale socket {}: {}",
                socket_path.as_ref().display(),
                e
            );
        }
    }

    fn build_startup_guard(
        &self,
        vm_id: VmId,
        active_socket_path: &str,
        tap_name: Option<String>,
        tap_ifindex: Option<u32>,
        chroot_dir: Option<String>,
    ) -> VmStartupGuard {
        let mut guard = VmStartupGuard::new(vm_id, PathBuf::from(active_socket_path));
        guard.tap_name = tap_name;
        guard.tap_ifindex = tap_ifindex;
        guard.chroot_dir = chroot_dir.map(PathBuf::from);
        guard.app_started = Arc::new(AtomicBool::new(false));
        guard.app_started_at_ms = Arc::new(AtomicU64::new(0));
        guard
    }

    async fn spawn_firecracker_process(
        &self,
        startup: &StartupContext,
    ) -> Result<tokio::process::Child, FirecrackerError> {
        tokio::process::Command::new(&startup.exec_binary)
            .args(&startup.exec_args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                let msg = format!(
                    "Failed to spawn firecracker process (binary: {}): {e}",
                    startup.exec_binary
                );
                tracing::error!("{}", msg);
                FirecrackerError::ProcessError(msg)
            })
    }

    async fn wait_for_firecracker_socket(
        &self,
        startup: &StartupContext,
    ) -> Result<(), FirecrackerError> {
        let wait_timeout = if startup.chroot_dir.is_some() {
            Duration::from_secs(10)
        } else {
            Duration::from_secs(5)
        };

        wait_for_socket(&startup.active_socket_path, wait_timeout).await?;
        Ok(())
    }

    fn mark_vm_app_started_now(&self, guard: &mut VmStartupGuard) {
        guard.app_started.store(true, Ordering::SeqCst);
        guard.app_started_at_ms.store(
            chrono::Utc::now().timestamp_millis() as u64,
            Ordering::SeqCst,
        );
    }

    async fn spawn_log_task(
        &self,
        vm_id: &VmId,
        app_id: &AppId,
        child: &mut tokio::process::Child,
        app_started: Arc<AtomicBool>,
        app_started_at_ms: Arc<AtomicU64>,
    ) -> tokio::task::JoinHandle<()> {
        let stdout = child.stdout.take().expect("Failed to take stdout");
        let stderr = child.stderr.take().expect("Failed to take stderr");

        let shipper = LogShipper::new(
            *vm_id,
            *app_id,
            self.nats_client.read().await.clone(),
            self.logs.clone(),
            app_started,
            app_started_at_ms,
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

        self.apply_machine_config(socket, config).await?;
        self.apply_boot_source(socket, config, kernel_path, chroot_dir)
            .await?;
        self.apply_root_drive(socket, rootfs_path, chroot_dir)
            .await?;
        self.apply_network_interface(socket, config, tap_name)
            .await?;
        self.apply_additional_volumes(socket, config, chroot_dir)
            .await?;
        self.start_instance(socket).await?;
        self.add_ipv6_host_route(config).await;
        Ok(())
    }

    async fn apply_machine_config(
        &self,
        socket: &str,
        config: &VmConfig,
    ) -> Result<(), FirecrackerError> {
        let machine_config = serde_json::json!({
            "vcpu_count": config.vcpus,
            "mem_size_mib": config.memory_mib,
            "smt": false,
            "track_dirty_pages": false
        })
        .to_string();
        fc_put(socket, "/machine-config", &machine_config).await?;
        Ok(())
    }

    fn build_boot_args(&self, config: &VmConfig) -> String {
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

        boot_args
    }

    async fn apply_boot_source(
        &self,
        socket: &str,
        config: &VmConfig,
        kernel_path: &str,
        chroot_dir: &Option<String>,
    ) -> Result<(), FirecrackerError> {
        let kernel_api_path = if chroot_dir.is_some() {
            "/vmlinux.bin".to_string()
        } else {
            kernel_path.to_string()
        };
        let boot_source = serde_json::json!({
            "kernel_image_path": kernel_api_path,
            "boot_args": self.build_boot_args(config)
        })
        .to_string();
        fc_put(socket, "/boot-source", &boot_source).await?;
        Ok(())
    }

    async fn apply_root_drive(
        &self,
        socket: &str,
        rootfs_path: &std::path::Path,
        chroot_dir: &Option<String>,
    ) -> Result<(), FirecrackerError> {
        let rootfs_api_path = if chroot_dir.is_some() {
            "/rootfs.ext4".to_string()
        } else {
            rootfs_path.to_string_lossy().to_string()
        };
        let drive_json = serde_json::json!({
            "drive_id": "rootfs",
            "path_on_host": rootfs_api_path,
            "is_root_device": true,
            "is_read_only": false
        })
        .to_string();
        fc_put(socket, "/drives/rootfs", &drive_json).await?;
        Ok(())
    }

    async fn apply_network_interface(
        &self,
        socket: &str,
        config: &VmConfig,
        tap_name: Option<&str>,
    ) -> Result<(), FirecrackerError> {
        if let Some(tap) = tap_name {
            let net_json = serde_json::json!({
                "iface_id": "eth0",
                "guest_mac": config.mac_address.as_deref().unwrap_or("AA:BB:CC:DD:EE:01"),
                "host_dev_name": tap
            })
            .to_string();
            fc_put(socket, "/network-interfaces/eth0", &net_json).await?;
        }
        Ok(())
    }

    async fn apply_additional_volumes(
        &self,
        socket: &str,
        config: &VmConfig,
        chroot_dir: &Option<String>,
    ) -> Result<(), FirecrackerError> {
        for vol in &config.volumes {
            let vol_host_path = self.ensure_volume(vol).await?;
            let vol_api_path = self
                .volume_api_path(vol, &vol_host_path, chroot_dir)
                .await?;
            let drive_id = vol.volume_id.replace('-', "_");
            let vol_json = serde_json::json!({
                "drive_id": drive_id,
                "path_on_host": vol_api_path,
                "is_root_device": false,
                "is_read_only": vol.read_only
            })
            .to_string();
            fc_put(socket, &format!("/drives/{}", drive_id), &vol_json).await?;
        }
        Ok(())
    }

    async fn volume_api_path(
        &self,
        vol: &crate::firecracker::config::Volume,
        vol_host_path: &str,
        chroot_dir: &Option<String>,
    ) -> Result<String, FirecrackerError> {
        if let Some(chroot) = chroot_dir {
            let filename = format!("{}.ext4", vol.volume_id);
            let c_path = format!("{chroot}/root/{filename}");

            if !vol.pool_name.is_empty() {
                // It's a block device, we need mknod
                self.mknod_at(vol_host_path, &c_path).await?;
            } else {
                self.ensure_file_at(vol_host_path, &c_path).await?;
            }

            self.recursive_chown(
                &c_path,
                self.fc_config.jailer_uid,
                self.fc_config.jailer_gid,
            )
            .await?;
            Ok(format!("/{filename}"))
        } else {
            Ok(vol_host_path.to_string())
        }
    }

    async fn start_instance(&self, socket: &str) -> Result<(), FirecrackerError> {
        // Firecracker applies API configuration asynchronously, so give it a brief moment
        // to settle before triggering the start action.
        tokio::time::sleep(Duration::from_millis(15)).await;
        fc_put(
            socket,
            "/actions",
            &serde_json::json!({ "action_type": "InstanceStart" }).to_string(),
        )
        .await?;
        Ok(())
    }

    async fn add_ipv6_host_route(&self, config: &VmConfig) {
        // This is a best-effort host route for direct guest reachability.
        if let Some(ipv6) = &config.ipv6_address
            && let Some(prefix_str) = Self::ipv6_route_prefix(ipv6)
            && let Ok(handle) = self.rtnl_handle().await
            && let Ok(Some(index)) = self.get_link_index(&handle, "mikrom-br0").await
        {
            let (addr, prefix) = match self.parse_ip_cidr(&prefix_str) {
                Ok(res) => res,
                Err(_) => return,
            };

            let req = handle.route().add().replace();
            let res = match addr {
                IpAddr::V4(v4) => {
                    req.v4()
                        .destination_prefix(v4, prefix)
                        .output_interface(index)
                        .execute()
                        .await
                },
                IpAddr::V6(v6) => {
                    req.v6()
                        .destination_prefix(v6, prefix)
                        .output_interface(index)
                        .execute()
                        .await
                },
            };

            if let Err(e) = res {
                tracing::warn!(
                    prefix = %prefix_str,
                    "Failed to add IPv6 host route for VM: {e}"
                );
            } else {
                tracing::info!(prefix = %prefix_str, "Added IPv6 host route for VM");
            }
        }
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

    pub async fn is_app_started(&self, vm_id: &VmId) -> bool {
        let processes = self.processes.lock().await;
        processes
            .get(vm_id)
            .map(|proc| proc.app_started.load(Ordering::SeqCst))
            .unwrap_or(false)
    }

    pub async fn get_vm_started_at_ms(&self, vm_id: &VmId) -> Option<u64> {
        let processes = self.processes.lock().await;
        processes
            .get(vm_id)
            .map(|proc| proc.app_started_at_ms.load(Ordering::SeqCst))
    }

    #[cfg(test)]
    pub async fn mark_vm_app_started(&self, vm_id: &VmId, started_at_ms: u64) -> bool {
        let processes = self.processes.lock().await;
        if let Some(proc) = processes.get(vm_id) {
            proc.app_started.store(true, Ordering::SeqCst);
            proc.app_started_at_ms
                .store(started_at_ms, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    #[cfg(test)]
    pub async fn seed_started_process_for_test(&self, vm_id: VmId, started_at_ms: u64) {
        let child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("sleep 5")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("failed to spawn test child");

        let log_task = tokio::spawn(async {});
        let mut processes = self.processes.lock().await;
        processes.insert(
            vm_id,
            VmProcess {
                vm_id,
                child,
                socket_path: "/tmp/test.sock".to_string(),
                metrics_path: None,
                tap_name: None,
                tap_ifindex: None,
                log_task,
                chroot_dir: None,
                app_started: Arc::new(AtomicBool::new(true)),
                app_started_at_ms: Arc::new(AtomicU64::new(started_at_ms)),
            },
        );
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
            self.stop_running_process(vm_id, &mut proc).await;
            self.cleanup_exited_process_artifacts(vm_id, &proc).await;
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

    async fn stop_running_process(&self, vm_id: &VmId, proc: &mut VmProcess) {
        proc.log_task.abort();

        tracing::info!(vm_id = %vm_id, "Sending kill signal to Firecracker process for stopping");
        if let Err(e) = proc.child.kill().await {
            tracing::error!(vm_id = %vm_id, "Failed to send kill signal to Firecracker: {}", e);
        }
        let _ = proc.child.wait().await;
        tracing::info!(vm_id = %vm_id, "Firecracker process terminated");
    }

    /// Completely purge all resources associated with a VM ID.
    /// This includes stopping it if it's running, and deleting all disk artifacts.
    #[tracing::instrument(skip(self), fields(vm_id = %vm_id))]
    pub async fn delete_vm(&self, vm_id: &VmId) -> Result<(), FirecrackerError> {
        tracing::info!("Purging all resources for VM");

        let ipv6_address = {
            let vms = self.vms.read().await;
            vms.get(vm_id).and_then(|vm| vm.config.ipv6_address.clone())
        };

        // 1. Stop the VM if it's running
        let _ = self.stop_vm(vm_id).await;

        // 1b. Remove the host route for the guest IPv6 prefix if it exists.
        // This keeps deleted apps from leaving a stale route behind on mikrom-br0.
        if let Some(ipv6) = ipv6_address.as_deref() {
            self.cleanup_ipv6_route(ipv6).await;
        }

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

        let chroot_dir = proc.chroot_dir.clone();
        let (host_snapshot_path, host_mem_path, snapshot_path, mem_path) =
            self.snapshot_paths(vm_id, chroot_dir.as_deref());
        let snapshot_body = Self::snapshot_create_body(
            &snapshot_path.to_string_lossy(),
            &mem_path.to_string_lossy(),
        );

        fc_put(&proc.socket_path, "/snapshot/create", &snapshot_body).await?;

        if chroot_dir.is_some() {
            let chroot_root = self.get_chroot_dir(vm_id).join("root");
            self.ensure_file_at(
                &chroot_root.join("vm.snapshot").to_string_lossy(),
                &host_snapshot_path.to_string_lossy(),
            )
            .await?;
            self.ensure_file_at(
                &chroot_root.join("vm.mem").to_string_lossy(),
                &host_mem_path.to_string_lossy(),
            )
            .await?;
        }

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
        let mut processes = self.processes.lock().await;
        let restart_from_snapshot = if let Some(proc) = processes.get_mut(vm_id) {
            match proc.child.try_wait() {
                Ok(Some(status)) => {
                    tracing::warn!(
                        vm_id = %vm_id,
                        status = ?status,
                        "Found stale Firecracker process during resume, restarting from snapshot"
                    );
                    true
                },
                Ok(None) => {
                    let resume_body = serde_json::json!({ "state": "Resumed" }).to_string();
                    match fc_patch(&proc.socket_path, "/vm", &resume_body).await {
                        Ok(_) => {
                            let mut vms = self.vms.write().await;
                            if let Some(vm) = vms.get_mut(vm_id) {
                                vm.status = VmStatus::Running;
                            }
                            return Ok(());
                        },
                        Err(e) => {
                            tracing::warn!(
                                vm_id = %vm_id,
                                error = %e,
                                "Failed to resume Firecracker process in place, restarting from snapshot"
                            );
                            true
                        },
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        vm_id = %vm_id,
                        error = %e,
                        "Could not inspect Firecracker process during resume, restarting from snapshot"
                    );
                    true
                },
            }
        } else {
            tracing::info!(vm_id = %vm_id, "Process missing for resume, attempting restart from snapshot...");
            true
        };

        if restart_from_snapshot {
            {
                let mut vms = self.vms.write().await;
                if let Some(vm) = vms.get_mut(vm_id) {
                    vm.status = VmStatus::Stopped;
                }
            }
            processes.remove(vm_id);
        }
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
        vol: &crate::firecracker::config::Volume,
    ) -> Result<String, FirecrackerError> {
        if !vol.pool_name.is_empty() {
            self.ensure_rbd_volume(vol).await
        } else {
            self.ensure_local_volume(vol).await
        }
    }

    async fn ensure_rbd_volume(
        &self,
        vol: &crate::firecracker::config::Volume,
    ) -> Result<String, FirecrackerError> {
        let storage = crate::ceph::CephRbd;
        if !storage.exists(&vol.pool_name, &vol.volume_id).await {
            storage
                .create_volume(&vol.pool_name, &vol.volume_id, vol.size_mib as i32)
                .await
                .map_err(|e| {
                    FirecrackerError::ProcessError(format!("Failed to create RBD volume: {e}"))
                })?;

            let dev_path = storage
                .map_volume(&vol.pool_name, &vol.volume_id)
                .await
                .map_err(|e| {
                    FirecrackerError::ProcessError(format!(
                        "Failed to map RBD volume for formatting: {e}"
                    ))
                })?;

            let output = tokio::process::Command::new("mkfs.ext4")
                .arg(&dev_path)
                .output()
                .await
                .map_err(|e| {
                    FirecrackerError::ProcessError(format!("Failed to execute mkfs.ext4: {e}"))
                })?;

            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr);
                let _ = storage.unmap_volume(&dev_path).await;
                return Err(FirecrackerError::ProcessError(format!(
                    "mkfs.ext4 failed: {err}"
                )));
            }

            storage.unmap_volume(&dev_path).await.map_err(|e| {
                FirecrackerError::ProcessError(format!(
                    "Failed to unmap RBD volume after formatting: {e}"
                ))
            })?;
        }

        storage
            .map_volume(&vol.pool_name, &vol.volume_id)
            .await
            .map_err(|e| FirecrackerError::ProcessError(format!("Failed to map RBD volume: {e}")))
    }

    async fn ensure_local_volume(
        &self,
        vol: &crate::firecracker::config::Volume,
    ) -> Result<String, FirecrackerError> {
        let vol_dir = format!("{}/volumes", self.fc_config.data_dir);
        tokio::fs::create_dir_all(&vol_dir).await.map_err(|e| {
            FirecrackerError::ProcessError(format!("Failed to create volumes dir: {e}"))
        })?;

        let vol_path = format!("{vol_dir}/{}.ext4", vol.volume_id);
        if tokio::fs::metadata(&vol_path).await.is_err() {
            let file = tokio::fs::File::create(&vol_path).await.map_err(|e| {
                FirecrackerError::ProcessError(format!("Failed to create volume file: {e}"))
            })?;
            file.set_len(vol.size_mib * 1024 * 1024)
                .await
                .map_err(|e| {
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

                    if !Self::is_active_resource_name(&file_name, &prefix, &active_vm_ids)
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

    fn is_active_resource_name(
        file_name: &str,
        prefix: &str,
        active_vm_ids: &std::collections::HashSet<VmId>,
    ) -> bool {
        active_vm_ids.iter().any(|vm_id| {
            let expected_socket = format!("{prefix}{vm_id}.sock");
            let expected_rootfs = format!("{prefix}{vm_id}-rootfs.ext4");
            let expected_metrics = format!("{prefix}{vm_id}-metrics.json");

            file_name == expected_socket
                || file_name == expected_rootfs
                || file_name == expected_metrics
        })
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

        let handle = self.rtnl_handle().await?;

        // 1. Create bridge if it doesn't exist
        if self.get_link_index(&handle, &bridge_name).await?.is_none() {
            handle
                .link()
                .add()
                .bridge(bridge_name.clone())
                .execute()
                .await
                .map_err(|e| {
                    FirecrackerError::ProcessError(format!(
                        "Failed to create bridge {}: {}",
                        bridge_name, e
                    ))
                })?;
        }

        let Some(index) = self.get_link_index(&handle, &bridge_name).await? else {
            return Err(FirecrackerError::ProcessError(format!(
                "Failed to find bridge {bridge_name} after creation"
            )));
        };

        // 2. Set MTU 1420 to match WireGuard overhead and avoid fragmentation
        self.set_link_mtu(&handle, index, 1420).await?;

        // 3. Set link UP
        self.set_link_up(&handle, index).await?;

        // 4. Add IPv4 address
        let (v4_addr, v4_prefix) = self.parse_ip_cidr(&bridge_ip)?;
        self.add_ip_address(&handle, index, v4_addr, v4_prefix)
            .await?;

        // 5. Add IPv6 addresses
        // Using /128 for the ULA address so it doesn't conflict with the WireGuard fd00::/8 route.
        self.add_ip_address(&handle, index, IpAddr::V6("fd00::1".parse().unwrap()), 128)
            .await?;

        self.add_ip_address(&handle, index, IpAddr::V6("fe80::1".parse().unwrap()), 64)
            .await?;

        // 6. Enable forwarding
        self.set_proc_sysctl("net/ipv4/ip_forward", "1").await?;
        self.set_proc_sysctl("net/ipv6/conf/all/forwarding", "1")
            .await?;
        self.set_proc_sysctl("net/ipv6/conf/default/forwarding", "1")
            .await?;
        self.set_proc_sysctl(&format!("net/ipv6/conf/{}/forwarding", bridge_name), "1")
            .await?;

        self.run_iptables_command([
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
        .await;

        Ok(())
    }

    async fn run_iptables_command<I, S>(&self, args: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        match tokio::process::Command::new("iptables")
            .args(args.into_iter().map(|arg| arg.as_ref().to_string()))
            .output()
            .await
        {
            Ok(output) if !output.status.success() => {
                tracing::warn!(
                    "Failed to setup iptables MASQUERADE: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            },
            Ok(_) => {},
            Err(error) => {
                tracing::warn!("Failed to execute iptables command: {}", error);
            },
        }
    }

    async fn set_proc_sysctl(&self, key: &str, value: &str) -> Result<(), FirecrackerError> {
        let path = format!("/proc/sys/{key}");
        tokio::fs::write(&path, value).await.map_err(|e| {
            FirecrackerError::ProcessError(format!(
                "Failed to write sysctl {key}={value} at {path}: {e}"
            ))
        })?;
        Ok(())
    }

    async fn setup_tap(&self, vm_id: &VmId) -> Result<(String, u32), FirecrackerError> {
        let tap_name = format!("m-tap-{}", &vm_id.to_string()[..8]);

        // 1. Create and configure TAP interface using native ioctl calls.
        // This sets the mode to TAP, disables packet info (NO_PI), makes it persistent,
        // and assigns ownership to the jailer's UID.
        self.create_tap_native(&tap_name, self.fc_config.jailer_uid)
            .map_err(|e| FirecrackerError::ProcessError(format!("Failed to create TAP: {e}")))?;

        let handle = self.rtnl_handle().await?;
        let Some(index) = self.get_link_index(&handle, &tap_name).await? else {
            return Err(FirecrackerError::ProcessError(format!(
                "TAP {tap_name} not found after native creation"
            )));
        };

        // 2. Set interface UP using rtnetlink
        self.set_link_up(&handle, index).await?;

        // 3. Attach to bridge and set MTU using rtnetlink
        let bridge_name = "mikrom-br0";
        let Some(bridge_index) = self.get_link_index(&handle, bridge_name).await? else {
            return Err(FirecrackerError::ProcessError(format!(
                "Bridge {bridge_name} not found"
            )));
        };

        handle
            .link()
            .set(index)
            .controller(bridge_index)
            .mtu(1420)
            .execute()
            .await
            .map_err(|e| {
                FirecrackerError::ProcessError(format!("Failed to attach TAP to bridge: {e}"))
            })?;

        Ok((tap_name, index))
    }

    fn create_tap_native(&self, name: &str, uid: u32) -> Result<(), String> {
        use std::os::unix::io::AsRawFd;
        let iface_name = CString::new(name).map_err(|e| e.to_string())?;

        // Open the TUN/TAP control device
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/net/tun")
            .map_err(|e| format!("Failed to open /dev/net/tun: {e}"))?;

        let fd = file.as_raw_fd();

        // Prepare the ifreq structure for TUNSETIFF
        let mut ifr: libc::ifreq = unsafe { std::mem::zeroed() };
        let name_bytes = iface_name.as_bytes();
        if name_bytes.len() >= ifr.ifr_name.len() {
            return Err("Interface name too long".to_string());
        }
        for (i, &byte) in name_bytes.iter().enumerate() {
            ifr.ifr_name[i] = byte as libc::c_char;
        }

        // Set flags: TAP mode and NO_PI (no extra packet information header)
        ifr.ifr_ifru.ifru_flags = (libc::IFF_TAP | libc::IFF_NO_PI) as i16;

        unsafe {
            // TUNSETIFF: Create or bind to the interface
            if libc::ioctl(fd, TUNSETIFF, &ifr) < 0 {
                return Err(format!(
                    "TUNSETIFF failed: {}",
                    std::io::Error::last_os_error()
                ));
            }

            // TUNSETOWNER: Set persistent owner UID
            if libc::ioctl(fd, TUNSETOWNER, uid as libc::c_ulong) < 0 {
                return Err(format!(
                    "TUNSETOWNER failed: {}",
                    std::io::Error::last_os_error()
                ));
            }

            // TUNSETPERSIST: Ensure the interface stays after we close the FD
            if libc::ioctl(fd, TUNSETPERSIST, 1) < 0 {
                return Err(format!(
                    "TUNSETPERSIST failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
        }

        Ok(())
    }

    async fn cleanup_tap(&self, tap_name: &str) {
        if let Ok(handle) = self.rtnl_handle().await
            && let Ok(Some(index)) = self.get_link_index(&handle, tap_name).await
        {
            // Remove from bridge (set nocontroller)
            let _ = handle.link().set(index).nocontroller().execute().await;
            // Delete link
            let _ = handle.link().del(index).execute().await;
        }
    }

    async fn cleanup_ipv6_route(&self, ipv6: &str) {
        let Some(prefix_str) = Self::ipv6_route_prefix(ipv6) else {
            return;
        };

        if let Ok(handle) = self.rtnl_handle().await {
            let (addr, prefix) = match self.parse_ip_cidr(&prefix_str) {
                Ok(res) => res,
                Err(_) => return,
            };

            let mut routes = handle.route().get(rtnetlink::IpVersion::V6).execute();
            while let Ok(Some(route)) = routes.try_next().await {
                if route.header.destination_prefix_length == prefix {
                    let dest = route.attributes.iter().find_map(|attr| match attr {
                        RouteAttribute::Destination(RouteAddress::Inet6(v6)) => {
                            Some(IpAddr::V6(*v6))
                        },
                        _ => None,
                    });

                    if dest == Some(addr) {
                        if let Err(e) = handle.route().del(route).execute().await {
                            tracing::debug!(prefix = %prefix_str, "Failed to delete route: {e}");
                        } else {
                            tracing::info!(prefix = %prefix_str, "Removed IPv6 host route");
                            return;
                        }
                    }
                }
            }
        }

        tracing::warn!(
            prefix = %prefix_str,
            "IPv6 host route may still be present after delete"
        );
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

        // Never hard-link the kernel into the jailer chroot.
        // The kernel must remain immutable on the host; otherwise chown/truncate
        // operations inside the chroot can affect the source file under /opt/firecracker.
        self.copy_file_at(kernel_host_path, &chroot_kernel_path.to_string_lossy())
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

    async fn validate_kernel_image(&self, kernel_path: &str) -> Result<(), FirecrackerError> {
        let mut kernel = tokio::fs::File::open(kernel_path).await.map_err(|e| {
            FirecrackerError::ProcessError(format!(
                "Failed to open kernel image at {kernel_path}: {e}"
            ))
        })?;
        let mut magic = [0u8; 4];
        kernel.read_exact(&mut magic).await.map_err(|e| {
            FirecrackerError::ProcessError(format!(
                "Failed to read kernel header at {kernel_path}: {e}"
            ))
        })?;

        if magic != [0x7f, b'E', b'L', b'F'] {
            return Err(FirecrackerError::ProcessError(format!(
                "Invalid kernel image at {kernel_path}: expected an uncompressed ELF Linux kernel, but the file does not start with ELF magic"
            )));
        }

        Ok(())
    }

    async fn mknod_at(&self, dev_path: &str, dst: &str) -> Result<(), FirecrackerError> {
        tracing::info!("Creating block device node: {} -> {}", dev_path, dst);

        let dev_path = dev_path.to_string();
        let dst = dst.to_string();
        tokio::task::spawn_blocking(move || -> Result<(), FirecrackerError> {
            let metadata = fs::metadata(&dev_path).map_err(|e| {
                FirecrackerError::ProcessError(format!("Failed to stat device {dev_path}: {e}"))
            })?;
            let dev = metadata.rdev();
            let major = libc::major(dev);
            let minor = libc::minor(dev);

            let path = CString::new(dst.as_str()).map_err(|e| {
                FirecrackerError::ProcessError(format!("Invalid device node path {dst}: {e}"))
            })?;
            let mode = libc::S_IFBLK | 0o600;
            let device = libc::makedev(major, minor);

            // SAFETY: mknod is a thin wrapper around the libc syscall. The path and
            // device values are derived from validated Rust values above.
            let rc = unsafe { libc::mknod(path.as_ptr(), mode, device) };
            if rc != 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::AlreadyExists {
                    return Ok(());
                }
                return Err(FirecrackerError::ProcessError(format!(
                    "mknod failed: {err}"
                )));
            }

            Ok(())
        })
        .await
        .map_err(|e| FirecrackerError::ProcessError(format!("Failed to create device node: {e}")))?
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

    async fn copy_file_at(&self, src: &str, dst: &str) -> Result<(), FirecrackerError> {
        let canonical_src = tokio::fs::canonicalize(src).await.map_err(|e| {
            FirecrackerError::ProcessError(format!("Failed to resolve path {src}: {e}"))
        })?;

        tokio::fs::copy(&canonical_src, dst).await.map_err(|e| {
            FirecrackerError::ProcessError(format!(
                "Failed to copy file from {canonical_src:?} to {dst}: {e}"
            ))
        })?;
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

    pub async fn set_vm_for_test(&self, vm_id: &VmId, vm_info: VmInfo) {
        self.vms.write().await.insert(*vm_id, vm_info);
    }

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
                app_started: Arc::new(AtomicBool::new(true)),
                app_started_at_ms: Arc::new(AtomicU64::new(
                    chrono::Utc::now().timestamp_millis() as u64
                )),
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

    pub(crate) async fn has_process(&self, vm_id: &VmId) -> bool {
        self.processes.lock().await.contains_key(vm_id)
    }
}
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::get_unwrap)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_ipv6_route_prefix_uses_guest_prefix() {
        let prefix = FirecrackerManager::ipv6_route_prefix("fd40:b90d:fcaa:ac99::1").unwrap();
        assert_eq!(prefix, "fd40:b90d:fcaa:ac99::/64");
    }

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

    fn temp_file_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mikrom-agent-{name}-{}-{}.bin",
            std::process::id(),
            uuid::Uuid::new_v4()
        ))
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

    #[test]
    fn test_snapshot_create_body_omits_version() {
        let body = FirecrackerManager::snapshot_create_body(
            "/var/lib/mikrom/data/snapshots/vm.snapshot",
            "/var/lib/mikrom/data/snapshots/vm.mem",
        );

        let parsed: serde_json::Value = serde_json::from_str(&body).expect("valid json");
        assert_eq!(
            parsed,
            serde_json::json!({
                "snapshot_type": "Full",
                "snapshot_path": "/var/lib/mikrom/data/snapshots/vm.snapshot",
                "mem_file_path": "/var/lib/mikrom/data/snapshots/vm.mem",
            })
        );
        assert!(parsed.get("version").is_none());
    }

    #[test]
    fn test_snapshot_paths_use_chroot_for_firecracker_when_jailer_is_enabled() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let vm_id = VmId::new();
        let (host_snapshot, host_mem, snapshot_path, mem_path) =
            mgr.snapshot_paths(&vm_id, Some("/srv/jailer/firecracker/test-vm"));

        assert_eq!(
            host_snapshot,
            std::path::Path::new("/tmp/mikrom-stub-data")
                .join("snapshots")
                .join(format!("{vm_id}.snapshot"))
        );
        assert_eq!(
            host_mem,
            std::path::Path::new("/tmp/mikrom-stub-data")
                .join("snapshots")
                .join(format!("{vm_id}.mem"))
        );
        assert_eq!(snapshot_path, std::path::PathBuf::from("/vm.snapshot"));
        assert_eq!(mem_path, std::path::PathBuf::from("/vm.mem"));
    }

    #[test]
    fn test_is_active_resource_name_matches_expected_artifacts() {
        let vm_id = VmId::new();
        let prefix = "fc-agent-";
        let mut active_vm_ids = std::collections::HashSet::new();
        active_vm_ids.insert(vm_id);

        assert!(FirecrackerManager::is_active_resource_name(
            &format!("{prefix}{vm_id}.sock"),
            prefix,
            &active_vm_ids
        ));
        assert!(FirecrackerManager::is_active_resource_name(
            &format!("{prefix}{vm_id}-rootfs.ext4"),
            prefix,
            &active_vm_ids
        ));
        assert!(FirecrackerManager::is_active_resource_name(
            &format!("{prefix}{vm_id}-metrics.json"),
            prefix,
            &active_vm_ids
        ));
        assert!(!FirecrackerManager::is_active_resource_name(
            &format!("{prefix}other.sock"),
            prefix,
            &active_vm_ids
        ));
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
    async fn test_resume_vm_restores_stopped_state_before_restart() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let vm_id = VmId::new();

        let child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("true")
            .spawn()
            .expect("failed to spawn test child");

        mgr.insert_process_for_test(&vm_id, child, "/tmp/fake-socket.sock".to_string())
            .await;
        mgr.set_vm_for_test(
            &vm_id,
            VmInfo {
                vm_id,
                app_id: AppId::new(),
                image: "test-image".to_string(),
                config: config(),
                status: VmStatus::Running,
                started_at: None,
                error_message: None,
            },
        )
        .await;

        mgr.resume_vm(&vm_id)
            .await
            .expect("resume_vm should restart from snapshot");

        assert!(!mgr.has_process(&vm_id).await);
        assert_ne!(mgr.get_vm_status(&vm_id).await.unwrap(), VmStatus::Running);
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
    async fn test_validate_kernel_image_accepts_elf_magic() {
        let kernel_path = temp_file_path("kernel-valid");
        tokio::fs::write(
            &kernel_path,
            [0x7f, b'E', b'L', b'F', 0x02, 0x01, 0x01, 0x00],
        )
        .await
        .unwrap();

        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        assert!(
            mgr.validate_kernel_image(kernel_path.to_str().unwrap())
                .await
                .is_ok()
        );

        let _ = tokio::fs::remove_file(&kernel_path).await;
    }

    #[tokio::test]
    async fn test_validate_kernel_image_rejects_non_elf_files() {
        let kernel_path = temp_file_path("kernel-invalid");
        tokio::fs::write(&kernel_path, b"not-a-kernel")
            .await
            .unwrap();

        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let err = mgr
            .validate_kernel_image(kernel_path.to_str().unwrap())
            .await
            .expect_err("expected invalid kernel error");
        assert!(err.to_string().contains("Invalid kernel image"));

        let _ = tokio::fs::remove_file(&kernel_path).await;
    }

    #[tokio::test]
    async fn test_copy_file_at_creates_distinct_inode() {
        use std::os::unix::fs::MetadataExt;

        let src_path = temp_file_path("kernel-src");
        let dst_path = temp_file_path("kernel-dst");
        tokio::fs::write(&src_path, b"kernel-bytes").await.unwrap();

        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        mgr.copy_file_at(src_path.to_str().unwrap(), dst_path.to_str().unwrap())
            .await
            .unwrap();

        let src_meta = tokio::fs::metadata(&src_path).await.unwrap();
        let dst_meta = tokio::fs::metadata(&dst_path).await.unwrap();

        assert_ne!(src_meta.ino(), dst_meta.ino());
        assert_eq!(tokio::fs::read(&src_path).await.unwrap(), b"kernel-bytes");
        assert_eq!(tokio::fs::read(&dst_path).await.unwrap(), b"kernel-bytes");

        let _ = tokio::fs::remove_file(&src_path).await;
        let _ = tokio::fs::remove_file(&dst_path).await;
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
        mgr.set_vm_for_test(
            &vm_id,
            VmInfo {
                vm_id,
                app_id: AppId::new(),
                image: "test-image".to_string(),
                config: VmConfig {
                    ipv6_address: Some("fd40:b90d:fcaa:ac99::42".to_string()),
                    ipv6_gateway: Some("fe80::1".to_string()),
                    ..config()
                },
                status: VmStatus::Stopped,
                started_at: None,
                error_message: None,
            },
        )
        .await;

        // 3. Perform delete
        mgr.delete_vm(&vm_id).await.expect("delete_vm failed");

        // 4. Verify everything is gone
        assert!(!rootfs.exists());
    }
}
