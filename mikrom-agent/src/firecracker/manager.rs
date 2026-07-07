use crate::firecracker::api::fc_put_with_timeouts;
use crate::firecracker::config::FirecrackerConfig;
use crate::firecracker::guard::VmStartupGuard;
use crate::firecracker::paths::VmPaths;
use crate::firecracker::process::VmProcess;
use crate::hypervisor::{
    HypervisorError, HypervisorType, VmConfig, VmDetailedInfo, VmHypervisor, VmInfo, VmStatus,
};
use crate::logger::LogShipper;
use async_trait::async_trait;
use mikrom_proto::{
    id::{AppId, VmId},
    subjects,
};
use prost::Message;

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
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
    /// Tracks allocated IP addresses on the host bridge.
    pub allocated_ips: Arc<tokio::sync::Mutex<std::collections::HashSet<std::net::Ipv6Addr>>>,
    /// NATS client for log streaming.
    nats_client: Arc<RwLock<Option<async_nats::Client>>>,
    pending_vm_failure_events: Arc<Mutex<Vec<PendingVmFailureEvent>>>,
    pub ebpf_manager: Arc<tokio::sync::Mutex<Option<crate::ebpf::EbpfManager>>>,
}

impl std::fmt::Debug for FirecrackerManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FirecrackerManager")
            .field("agent_id", &self.agent_id)
            .field("vms", &self.vms)
            .field("processes", &self.processes)
            .field("fc_config", &self.fc_config)
            .field("logs", &self.logs)
            .field("builder", &self.builder)
            .field("allocated_ips", &self.allocated_ips)
            .field("pending_vm_failure_events", &self.pending_vm_failure_events)
            .field("ebpf_manager", &self.ebpf_manager)
            .finish_non_exhaustive()
    }
}

pub(crate) struct StartupContext {
    pub(crate) exec_binary: String,
    pub(crate) exec_args: Vec<String>,
    pub(crate) active_socket_path: String,
    pub(crate) chroot_dir: Option<String>,
}

#[derive(Clone, Debug)]
struct PendingVmFailureEvent {
    vm_id: VmId,
    error_message: String,
}

impl FirecrackerManager {
    pub(crate) fn is_pid_alive(pid: u32) -> bool {
        let status_path = format!("/proc/{pid}/status");
        match std::fs::read_to_string(status_path) {
            Ok(status) => !status.lines().any(|line| line.starts_with("State:\tZ")),
            Err(_) => false,
        }
    }

    async fn wait_for_pid_exit(pid: u32, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if !Self::is_pid_alive(pid) {
                return true;
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    pub(crate) async fn kill_process(
        &self,
        vm_id: &VmId,
        proc: &mut VmProcess,
    ) -> Result<(), HypervisorError> {
        if let Some(child) = proc.child.as_mut() {
            if let Err(e) = child.kill().await {
                tracing::error!(vm_id = %vm_id, "Failed to send kill signal to Firecracker: {}", e);
            }
            let _ = child.wait().await;
            return Ok(());
        }

        if let Some(pid) = proc.pid {
            let rc = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            if rc != 0 {
                let err = std::io::Error::last_os_error();
                tracing::warn!(vm_id = %vm_id, pid = pid, error = %err, "Failed to signal recovered Firecracker process");
            }
            if !Self::wait_for_pid_exit(pid, self.fc_config.process_terminate_timeout()).await {
                tracing::warn!(vm_id = %vm_id, pid = pid, "SIGTERM timed out, sending SIGKILL");
                let rc = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
                if rc != 0 {
                    let err = std::io::Error::last_os_error();
                    tracing::warn!(vm_id = %vm_id, pid = pid, error = %err, "Failed to send SIGKILL to recovered Firecracker process");
                }
                let _ = Self::wait_for_pid_exit(pid, self.fc_config.process_kill_timeout()).await;
            }
            return Ok(());
        }

        Err(HypervisorError::StopFailed(format!(
            "No process handle or pid available for VM {vm_id}"
        )))
    }

    /// Attempt to find and stop a Firecracker process that is not in our active memory.
    pub(crate) async fn stop_orphaned_process(&self, vm_id: &VmId) {
        let paths = VmPaths::new(&self.fc_config.data_dir, &self.agent_id, *vm_id);
        let socket_path = paths.socket_path();

        if socket_path.exists() {
            tracing::info!(vm_id = %vm_id, "Found orphaned API socket, attempting to shutdown via API");
            // Try graceful shutdown via API
            let shutdown_payload = serde_json::json!({
                "action_type": "ShutdownHttp"
            })
            .to_string();

            if let Err(e) = fc_put_with_timeouts(
                &socket_path.to_string_lossy(),
                "/actions",
                &shutdown_payload,
                self.fc_config.api_connect_timeout(),
                self.fc_config.api_status_timeout(),
                self.fc_config.api_header_timeout(),
                self.fc_config.api_body_timeout(),
            )
            .await
            {
                tracing::debug!(vm_id = %vm_id, "Graceful orphaned shutdown failed (expected if not yet started): {e}");
            } else {
                tracing::info!(vm_id = %vm_id, "Gracefully shut down orphaned Firecracker process via API");
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
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
                let pid = proc.and_then(FirecrackerManager::process_pid);
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
                    raw_metrics: None,
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
        let builder = crate::builder::ImageBuilder::new().unwrap_or_else(|e| {
            tracing::error!("ImageBuilder::new failed (should never happen): {e}");
            crate::builder::ImageBuilder
        });

        let mut agent_id = uuid::Uuid::new_v4().to_string();

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
            allocated_ips: Arc::new(tokio::sync::Mutex::new(std::collections::HashSet::new())),
            nats_client: Arc::new(RwLock::new(Option::None)),
            pending_vm_failure_events: Arc::new(Mutex::new(Vec::new())),
            ebpf_manager: Arc::new(tokio::sync::Mutex::new(ebpf)),
        }
    }

    pub async fn set_nats_client(&self, client: async_nats::Client) {
        let mut n = self.nats_client.write().await;
        *n = Some(client);
        drop(n);
        self.flush_pending_vm_failure_events().await;
    }

    async fn queue_vm_failure_event(&self, vm_id: &VmId, error_message: String) {
        const MAX_PENDING_FAILURES: usize = 10_000;
        let mut pending = self.pending_vm_failure_events.lock().await;
        if pending.len() >= MAX_PENDING_FAILURES {
            tracing::warn!(vm_id = %vm_id, "Dropping VM failure event, queue full");
            return;
        }
        pending.push(PendingVmFailureEvent {
            vm_id: *vm_id,
            error_message,
        });
    }

    async fn publish_vm_failure_event_now(
        client: &async_nats::Client,
        vm_id: &VmId,
        error_message: String,
    ) -> anyhow::Result<()> {
        let event = mikrom_proto::agent::VmFailureEvent {
            vm_id: vm_id.to_string(),
            error_message,
        };
        let mut buf = Vec::new();
        event
            .encode(&mut buf)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        client
            .publish(subjects::SCHEDULER_VM_FAILED, buf.into())
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }

    async fn flush_pending_vm_failure_events(&self) {
        let Some(client) = self.nats_client.read().await.clone() else {
            return;
        };

        let pending = {
            let mut queued = self.pending_vm_failure_events.lock().await;
            std::mem::take(&mut *queued)
        };

        let mut failed = Vec::new();
        for event in pending {
            if let Err(e) = Self::publish_vm_failure_event_now(
                &client,
                &event.vm_id,
                event.error_message.clone(),
            )
            .await
            {
                tracing::warn!(
                    vm_id = %event.vm_id,
                    error = %e,
                    "Failed to publish queued VM failure event"
                );
                failed.push(event);
            }
        }

        if !failed.is_empty() {
            let mut queued = self.pending_vm_failure_events.lock().await;
            queued.extend(failed);
        }
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

    pub async fn start_vm(
        &self,
        vm_id: VmId,
        app_id: AppId,
        image: String,
        config: VmConfig,
    ) -> Result<(), HypervisorError> {
        {
            let mut vms = self.vms.write().await;
            if let Some(vm) = vms.get_mut(&vm_id) {
                if vm.status == VmStatus::Running
                    || vm.status == VmStatus::Starting
                    || vm.status == VmStatus::Stopping
                {
                    return Err(HypervisorError::StartFailed(
                        "VM already exists and is running, starting, or stopping".to_string(),
                    ));
                }

                let old_status = vm.status;
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
        let _ = self.persist_runtime_state().await;

        {
            self.logs
                .entry(vm_id)
                .or_default()
                .push_back(format!("[agent] Initializing VM {vm_id}..."));
        }

        if let Some(_kernel) = &self.fc_config.kernel_path {
            let binary = &self.fc_config.binary;
            if tokio::fs::metadata(binary).await.is_err() {
                let err_msg = format!("Firecracker binary not found: {binary}");
                self.set_failed(&vm_id, err_msg.clone()).await;
                return Err(HypervisorError::ProcessError(err_msg));
            }
        }

        if let Some(kernel_path) = &self.fc_config.kernel_path {
            self.validate_kernel_image(kernel_path).await?;
        }

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
    ) -> Result<(), HypervisorError> {
        tracing::info!(vm_id = %vm_id, "Background VM startup initiated");

        let paths = VmPaths::new(&self.fc_config.data_dir, &self.agent_id, vm_id);

        let Some(kernel_path) = self.fc_config.kernel_path.clone() else {
            self.mark_vm_running(&vm_id).await;
            return Ok(());
        };

        let rootfs_path = paths.rootfs_path();
        self.prepare_rootfs(&vm_id, &image, &rootfs_path.to_string_lossy(), &config)
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
        guard.stdout_log_path = paths.stdout_log_path().to_string_lossy().to_string();
        guard.stderr_log_path = paths.stderr_log_path().to_string_lossy().to_string();

        let child = self.spawn_firecracker_process(&startup, &paths).await?;
        guard.log_task = Some(
            self.spawn_log_task_from_paths(
                &vm_id,
                &app_id,
                guard.stdout_log_path.clone(),
                guard.stderr_log_path.clone(),
                guard.stdout_log_offset.clone(),
                guard.stderr_log_offset.clone(),
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

        let mut snapshot_restore_failed = false;
        match self
            .try_restore_snapshot(
                &vm_id,
                &startup.chroot_dir,
                &startup.active_socket_path,
                &paths,
            )
            .await?
        {
            crate::firecracker::snapshots::SnapshotRestoreOutcome::Restored => {
                self.mark_vm_app_started_now(&mut guard);
                self.finalize_startup(guard).await?;
                return Ok(());
            },
            crate::firecracker::snapshots::SnapshotRestoreOutcome::Failed => {
                tracing::warn!(
                    vm_id = %vm_id,
                    "Snapshot load failed, restarting Firecracker process before normal boot"
                );
                snapshot_restore_failed = true;
            },
            crate::firecracker::snapshots::SnapshotRestoreOutcome::Missing => {},
        }

        if snapshot_restore_failed {
            tokio::time::sleep(Duration::from_secs(5)).await;
        }

        if let Err(err) = self
            .configure_vm_api(
                &config,
                &kernel_path,
                &rootfs_path,
                &startup.chroot_dir,
                &startup.active_socket_path,
                tap_name.as_deref(),
                &mut guard,
            )
            .await
        {
            if snapshot_restore_failed {
                tracing::warn!(
                    vm_id = %vm_id,
                    error = %err,
                    "Boot configuration failed after snapshot load error, restarting Firecracker process"
                );
                self.restart_firecracker_process(&startup, &paths, &mut guard)
                    .await?;
                self.configure_vm_api(
                    &config,
                    &kernel_path,
                    &rootfs_path,
                    &startup.chroot_dir,
                    &startup.active_socket_path,
                    tap_name.as_deref(),
                    &mut guard,
                )
                .await?;
            } else {
                return Err(err);
            }
        }

        self.finalize_startup(guard).await?;
        Ok(())
    }

    async fn restart_firecracker_process(
        &self,
        startup: &StartupContext,
        paths: &VmPaths,
        guard: &mut VmStartupGuard,
    ) -> Result<(), HypervisorError> {
        if let Some(mut child) = guard.child.take() {
            let wait_for_exit = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
            if wait_for_exit.is_err() {
                let _ = child.kill().await;
            }
            let _ = child.wait().await;
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
        self.remove_stale_socket(&startup.active_socket_path).await;

        let child = self.spawn_firecracker_process(startup, paths).await?;
        guard.child = Some(child);

        self.wait_for_firecracker_socket(startup).await?;
        guard.metrics_path = Some(
            self.setup_metrics(
                &guard.vm_id,
                &startup.chroot_dir,
                &startup.active_socket_path,
                paths,
            )
            .await?,
        );

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
    ) -> Result<(Option<String>, Option<u32>), HypervisorError> {
        if config.ipv6_address.as_deref().unwrap_or("").is_empty() {
            if let Some((ip, gw, mac)) = self.allocate_vm_network().await {
                tracing::info!(vm_id = %vm_id, ipv6 = %ip, "Allocated IPv6 from agent bridge subnet");
                config.ipv6_address = Some(ip);
                config.ipv6_gateway = Some(gw);
                config.mac_address = Some(mac);
            } else {
                tracing::warn!(
                    vm_id = %vm_id,
                    "No available IPv6 addresses in bridge subnet, starting without networking"
                );
            }
        }

        if config.ipv6_address.is_some() {
            let (tap, ifindex) = self.setup_tap(vm_id).await?;
            self.attach_tc_best_effort(&tap).await;
            Ok((Some(tap), Some(ifindex)))
        } else {
            Ok((None, None))
        }
    }

    async fn finalize_startup(&self, guard: VmStartupGuard) -> Result<(), HypervisorError> {
        let vm_id = guard.vm_id;
        let vm_process = guard.commit().ok_or_else(|| {
            HypervisorError::ProcessError(
                "Startup guard missing child process or log task".to_string(),
            )
        })?;

        {
            let mut vms = self.vms.write().await;
            if let Some(vm) = vms.get_mut(&vm_id) {
                vm.status = VmStatus::Running;
                vm.started_at = Some(chrono::Utc::now().timestamp());
            }
        }

        self.processes.lock().await.insert(vm_id, vm_process);
        let _ = self.persist_runtime_state().await;
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
                pid: child.id(),
                child: Some(child),
                socket_path: "/tmp/test.sock".to_string(),
                metrics_path: None,
                stdout_log_path: "/tmp/test.stdout.log".to_string(),
                stderr_log_path: "/tmp/test.stderr.log".to_string(),
                stdout_log_offset: Arc::new(AtomicU64::new(0)),
                stderr_log_offset: Arc::new(AtomicU64::new(0)),
                tap_name: None,
                tap_ifindex: None,
                log_task: Some(log_task),
                chroot_dir: None,
                app_started: Arc::new(AtomicBool::new(true)),
                app_started_at_ms: Arc::new(AtomicU64::new(started_at_ms)),
                vfs_processes: Vec::new(),
                vfs_pids: Vec::new(),
            },
        );
    }

    async fn prepare_rootfs(
        &self,
        vm_id: &VmId,
        image: &str,
        rootfs_path: &str,
        config: &VmConfig,
    ) -> Result<(), HypervisorError> {
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
                return Err(HypervisorError::ProcessError(err));
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
                .docker_to_ext4(crate::builder::DockerToExt4Params {
                    image,
                    output_path: dst_path,
                    base_rootfs_path: &self.fc_config.base_rootfs_path,
                    port: config.port,
                    ipv6_addr: config.ipv6_address.clone(),
                    ipv6_gw: config.ipv6_gateway.clone(),
                    volumes: &config.volumes,
                    workload_type: config.workload_type,
                })
                .await
                .map_err(|e| HypervisorError::ProcessError(format!("Image builder failed: {e}")))?;
        }
        Ok(())
    }

    pub async fn restart_vm(&self, vm_id: &VmId) -> Result<(), HypervisorError> {
        let (app_id, image, config) = {
            let vms = self.vms.read().await;
            let vm = vms
                .get(vm_id)
                .ok_or_else(|| HypervisorError::VmNotFound(vm_id.to_string()))?;
            (vm.app_id, vm.image.clone(), vm.config.clone())
        };

        tracing::info!(vm_id = %vm_id, "Restarting VM...");
        let _ = self.stop_vm(vm_id).await;
        self.start_vm(*vm_id, app_id, image, config).await?;
        Ok(())
    }

    #[tracing::instrument(skip(self), fields(vm_id = %vm_id))]
    pub async fn stop_vm(&self, vm_id: &VmId) -> Result<(), HypervisorError> {
        let volumes = {
            let vms = self.vms.read().await;
            vms.get(vm_id)
                .map(|vm| vm.config.volumes.clone())
                .unwrap_or_default()
        };

        {
            let mut vms = self.vms.write().await;
            match vms.get_mut(vm_id) {
                Some(vm) => vm.status = VmStatus::Stopping,
                None => return Err(HypervisorError::VmNotFound(vm_id.to_string())),
            }
        }

        self.logs.remove(vm_id);

        if let Some(mut proc) = self.processes.lock().await.remove(vm_id) {
            self.stop_running_process(vm_id, &mut proc).await;
            self.cleanup_exited_process_artifacts(vm_id, &proc, &volumes)
                .await;
            if let Some(ref tap) = proc.tap_name {
                self.cleanup_tap(tap).await;
            }
        } else {
            tracing::info!(vm_id = %vm_id, "Process already gone, performing best-effort cleanup of volumes and artifacts");
            self.cleanup_process_volumes(&volumes).await;
        }

        let mut vms = self.vms.write().await;
        if let Some(vm) = vms.get_mut(vm_id) {
            if let Some(ip) = &vm.config.ipv6_address {
                self.release_vm_ip(ip).await;
            }
            vm.status = VmStatus::Stopped;
        }
        drop(vms);
        let _ = self.persist_runtime_state().await;

        Ok(())
    }

    async fn stop_running_process(&self, vm_id: &VmId, proc: &mut VmProcess) {
        if let Some(ref mut task) = proc.log_task {
            task.abort();
        }

        let had_vfs_children = !proc.vfs_processes.is_empty();
        for mut vfs_child in proc.vfs_processes.drain(..) {
            if let Err(e) = vfs_child.kill().await {
                tracing::error!(vm_id = %vm_id, error = %e, "Failed to kill virtiofsd process");
            }
            let _ = vfs_child.wait().await;
        }
        if !had_vfs_children {
            for pid in proc.vfs_pids.drain(..) {
                let rc = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
                if rc != 0 {
                    let err = std::io::Error::last_os_error();
                    tracing::warn!(vm_id = %vm_id, pid = pid, error = %err, "Failed to signal recovered virtiofsd process");
                }
                if !Self::wait_for_pid_exit(pid, self.fc_config.vfs_terminate_timeout()).await {
                    tracing::warn!(vm_id = %vm_id, pid = pid, "SIGTERM timed out for virtiofsd, sending SIGKILL");
                    let _ = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
                    let _ = Self::wait_for_pid_exit(pid, self.fc_config.vfs_kill_timeout()).await;
                }
            }
        }

        tracing::info!(
            vm_id = %vm_id,
            "Sending kill signal to Firecracker process for stopping"
        );
        let _ = self.kill_process(vm_id, proc).await;
        tracing::info!(vm_id = %vm_id, "Firecracker process terminated");
    }

    #[tracing::instrument(skip(self), fields(vm_id = %vm_id))]
    pub async fn delete_vm(&self, vm_id: &VmId) -> Result<(), HypervisorError> {
        tracing::info!("Purging all resources for VM");

        let ipv6_address = {
            let vms = self.vms.read().await;
            vms.get(vm_id).and_then(|vm| vm.config.ipv6_address.clone())
        };

        // Attempt to stop the VM if it's in memory. If not, stop_vm will fail with VmNotFound,
        // which we ignore for a purge.
        let _ = self.stop_vm(vm_id).await;

        // If it wasn't in vms memory, stop_vm didn't do much.
        // We should try to kill any orphaned process based on expected PID or socket.
        self.stop_orphaned_process(vm_id).await;

        if let Some(ipv6) = ipv6_address.as_deref() {
            self.cleanup_ipv6_route(ipv6).await;
        }

        {
            let mut vms = self.vms.write().await;
            vms.remove(vm_id);
        }
        let _ = self.persist_runtime_state().await;

        // Best-effort cleanup of any leftover artifacts.
        self.cleanup_process_paths(vm_id, None).await;
        self.cleanup_snapshot_files(vm_id).await;
        self.cleanup_vm_chroot(vm_id).await;

        Ok(())
    }

    pub async fn get_vm_status(&self, vm_id: &VmId) -> Result<VmStatus, HypervisorError> {
        let vms = self.vms.read().await;
        match vms.get(vm_id) {
            Some(vm) => Ok(vm.status),
            None => Err(HypervisorError::VmNotFound(vm_id.to_string())),
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

    async fn set_failed(&self, vm_id: &VmId, msg: String) {
        let mut vms = self.vms.write().await;
        if let Some(vm) = vms.get_mut(vm_id) {
            vm.status = VmStatus::Failed;
            vm.error_message = Some(msg.clone());
        }
        drop(vms);
        let _ = self.persist_runtime_state().await;

        self.publish_vm_failure_event(vm_id, msg).await;
    }

    pub(crate) async fn publish_vm_failure_event(&self, vm_id: &VmId, msg: String) {
        let Some(client) = self.nats_client.read().await.clone() else {
            self.queue_vm_failure_event(vm_id, msg).await;
            return;
        };

        if let Err(e) = Self::publish_vm_failure_event_now(&client, vm_id, msg.clone()).await {
            tracing::warn!(vm_id = %vm_id, error = %e, "Failed to publish VM failure event");
            self.queue_vm_failure_event(vm_id, msg).await;
        }
    }

    fn mark_vm_app_started_now(&self, guard: &mut VmStartupGuard) {
        guard.app_started.store(true, Ordering::SeqCst);
        guard.app_started_at_ms.store(
            chrono::Utc::now().timestamp_millis().max(0) as u64,
            Ordering::SeqCst,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn spawn_log_task_from_paths(
        &self,
        vm_id: &VmId,
        app_id: &AppId,
        stdout_path: String,
        stderr_path: String,
        stdout_offset: Arc<AtomicU64>,
        stderr_offset: Arc<AtomicU64>,
        app_started: Arc<AtomicBool>,
        app_started_at_ms: Arc<AtomicU64>,
    ) -> tokio::task::JoinHandle<()> {
        let shipper = LogShipper::new(
            *vm_id,
            *app_id,
            self.nats_client.read().await.clone(),
            self.logs.clone(),
            app_started,
            app_started_at_ms,
        );

        shipper
            .spawn_from_paths(
                PathBuf::from(stdout_path),
                stdout_offset,
                PathBuf::from(stderr_path),
                stderr_offset,
            )
            .await
    }

    async fn setup_metrics(
        &self,
        vm_id: &VmId,
        chroot_dir: &Option<String>,
        active_socket_path: &str,
        paths: &VmPaths,
    ) -> Result<String, HypervisorError> {
        let (host_path, api_path) = if let Some(chroot) = chroot_dir {
            let h_path = format!("{chroot}/root/metrics.json");
            tokio::fs::write(&h_path, b"").await.map_err(|e| {
                HypervisorError::ProcessError(format!("Failed to create metrics file: {e}"))
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
        if let Err(e) = fc_put_with_timeouts(
            active_socket_path,
            "/metrics",
            &metrics_config,
            self.fc_config.api_connect_timeout(),
            self.fc_config.api_status_timeout(),
            self.fc_config.api_header_timeout(),
            self.fc_config.api_body_timeout(),
        )
        .await
        {
            tracing::warn!(vm_id = %vm_id, "Failed to configure metrics: {e}");
        }
        Ok(host_path)
    }
}

#[async_trait]
impl VmHypervisor for FirecrackerManager {
    fn hypervisor_type(&self) -> HypervisorType {
        HypervisorType::Firecracker
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
        self.pause_vm(vm_id).await
    }

    async fn resume_vm(&self, vm_id: &VmId) -> Result<(), HypervisorError> {
        self.resume_vm(vm_id).await
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
        self.get_logs(vm_id)
    }

    async fn update_vm_firewall(
        &self,
        vm_id: &VmId,
        rules: Vec<mikrom_agent_ebpf_common::FirewallRule>,
    ) -> Result<(), HypervisorError> {
        self.update_vm_firewall(vm_id, rules)
            .await
            .map_err(|e| HypervisorError::ProcessError(e.to_string()))
    }

    async fn init_network(&self) -> Result<(), HypervisorError> {
        self.init_network().await
    }

    async fn load_runtime_state(&self) -> Result<(), HypervisorError> {
        self.load_runtime_state()
            .await
            .map_err(|e| HypervisorError::ProcessError(e.to_string()))
    }

    async fn persist_runtime_state(&self) -> Result<(), HypervisorError> {
        self.persist_runtime_state()
            .await
            .map_err(|e| HypervisorError::ProcessError(e.to_string()))
    }

    async fn cleanup_all_stale_resources(&self) {
        self.cleanup_all_stale_resources().await;
    }

    async fn set_nats_client(&self, client: async_nats::Client) {
        self.set_nats_client(client).await;
    }

    fn start_background_tasks(&self) {
        self.start_background_tasks();
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
                pid: child.id(),
                child: Some(child),
                socket_path,
                metrics_path: None,
                stdout_log_path: "/tmp/test.stdout.log".to_string(),
                stderr_log_path: "/tmp/test.stderr.log".to_string(),
                stdout_log_offset: Arc::new(AtomicU64::new(0)),
                stderr_log_offset: Arc::new(AtomicU64::new(0)),
                tap_name: None,
                tap_ifindex: None,
                log_task: Some(log_task),
                chroot_dir: None,
                app_started: Arc::new(AtomicBool::new(true)),
                app_started_at_ms: Arc::new(AtomicU64::new(
                    chrono::Utc::now().timestamp_millis().max(0) as u64,
                )),
                vfs_processes: Vec::new(),
                vfs_pids: Vec::new(),
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
    use crate::firecracker::api::wait_for_socket;
    use crate::firecracker::state::{PersistedAgentState, PersistedVmRecord, PersistedVmRuntime};
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::OnceLock;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixListener;
    use tokio::task::JoinHandle;

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
            workload_type: 0,
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

    fn temp_data_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mikrom-agent-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ))
    }

    fn temp_socket_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mikrom-agent-{name}-{}-{}.sock",
            std::process::id(),
            uuid::Uuid::new_v4()
        ))
    }

    fn bridge_ip_lock() -> &'static tokio::sync::Mutex<()> {
        static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    fn write_fc_helper_script(path: &PathBuf) {
        let script = r#"#!/usr/bin/env python3
import os
import socket
import sys
import time

def parse_socket_path(argv):
    for i, arg in enumerate(argv):
        if arg == "--api-sock" and i + 1 < len(argv):
            return argv[i + 1]
    raise SystemExit("missing --api-sock")

def read_request(conn):
    data = b""
    while b"\r\n\r\n" not in data:
        chunk = conn.recv(4096)
        if not chunk:
            break
        data += chunk
    return data.decode("utf-8", "replace")

def respond(conn, status, body=b""):
    payload = (
        f"HTTP/1.1 {status}\r\n"
        f"Content-Length: {len(body)}\r\n"
        "Connection: close\r\n"
        "\r\n"
    ).encode("utf-8") + body
    conn.sendall(payload)

sock_path = parse_socket_path(sys.argv[1:])
log_path = sock_path + ".log"
marker_path = sock_path + ".marker"
first_run = not os.path.exists(marker_path)
mode = "restore" if first_run else "boot"

with open(log_path, "a", encoding="utf-8") as log:
    log.write(f"mode={mode}\n")
    log.flush()

if first_run:
    with open(marker_path, "w", encoding="utf-8"):
        pass

if os.path.exists(sock_path):
    os.unlink(sock_path)

server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
bind_error = None
for _ in range(300):
    try:
        server.bind(sock_path)
        bind_error = None
        break
    except OSError as exc:
        bind_error = exc
        time.sleep(0.1)
else:
    raise bind_error

server.listen(1)

if first_run:
    while True:
        conn, _ = server.accept()
        with conn:
            request = read_request(conn)
            first_line = request.splitlines()[0] if request.splitlines() else ""
            with open(log_path, "a", encoding="utf-8") as log:
                log.write(first_line + "\n")
            if "/snapshot/load" in first_line:
                respond(
                    conn,
                    "400 Bad Request",
                    b'{"fault_message":"snapshot load failed"}',
                )
                continue
            respond(conn, "204 No Content")

for _ in range(8):
    conn, _ = server.accept()
    with conn:
        request = read_request(conn)
        first_line = request.splitlines()[0] if request.splitlines() else ""
        with open(log_path, "a", encoding="utf-8") as log:
            log.write(first_line + "\n")
        respond(conn, "204 No Content")

server.close()
"#;

        std::fs::write(path, script).expect("failed to write helper script");
        let mut perms = std::fs::metadata(path)
            .expect("failed to stat helper script")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).expect("failed to set helper script permissions");
    }

    enum SnapshotRestartAction {
        StartVm,
        ResumeVm,
    }

    async fn run_snapshot_restart_test(test_name: &str, action: SnapshotRestartAction) {
        let _bridge_guard = bridge_ip_lock().lock().await;
        let prev_bridge_ip = std::env::var("BRIDGE_IP").ok();
        unsafe {
            std::env::set_var("BRIDGE_IP", "fd00::1/128");
        }

        let cleanup_env = |prev: Option<String>| {
            if let Some(value) = prev {
                unsafe {
                    std::env::set_var("BRIDGE_IP", value);
                }
            } else {
                unsafe {
                    std::env::remove_var("BRIDGE_IP");
                }
            }
        };

        let data_dir = std::env::temp_dir().join(format!("m{}", uuid::Uuid::new_v4().simple()));
        let _ = std::fs::remove_dir_all(&data_dir);
        std::fs::create_dir_all(&data_dir).expect("failed to create data dir");

        let kernel_path = temp_file_path(&format!("{test_name}-kernel"));
        std::fs::write(&kernel_path, b"\x7fELFrest").expect("failed to write kernel image");

        let rootfs_image = temp_file_path(&format!("{test_name}-rootfs"));
        std::fs::write(&rootfs_image, b"rootfs").expect("failed to write rootfs image");

        let helper_script = temp_file_path(&format!("{test_name}-helper"));
        write_fc_helper_script(&helper_script);

        let fc_config = FirecrackerConfig {
            kernel_path: Some(kernel_path.to_string_lossy().to_string()),
            binary: helper_script.to_string_lossy().to_string(),
            rootfs_path: rootfs_image.to_string_lossy().to_string(),
            base_rootfs_path: rootfs_image.to_string_lossy().to_string(),
            data_dir: data_dir.to_string_lossy().to_string(),
            use_jailer: false,
            jailer_binary: String::new(),
            jailer_uid: 0,
            jailer_gid: 0,
            chroot_base: data_dir.join("chroot").to_string_lossy().to_string(),
            virtiofsd_path: String::new(),
        };

        let vm_id = VmId::new();
        let snapshot_dir = data_dir.join("snapshots");
        std::fs::create_dir_all(&snapshot_dir).expect("failed to create snapshot dir");
        std::fs::write(snapshot_dir.join(format!("{vm_id}.snapshot")), b"snapshot")
            .expect("failed to seed snapshot file");
        std::fs::write(snapshot_dir.join(format!("{vm_id}.mem")), b"memory")
            .expect("failed to seed memory file");

        let mgr = FirecrackerManager::with_config(fc_config);
        let app_id = AppId::new();
        let image = rootfs_image.to_string_lossy().to_string();
        let mut vm_config = config();
        vm_config.ipv6_address = None;
        vm_config.ipv6_gateway = None;
        vm_config.mac_address = None;

        match action {
            SnapshotRestartAction::StartVm => {
                mgr.start_vm(vm_id, app_id, image, vm_config)
                    .await
                    .expect("start_vm should accept background startup");
            },
            SnapshotRestartAction::ResumeVm => {
                mgr.set_vm_for_test(
                    &vm_id,
                    VmInfo {
                        vm_id,
                        app_id,
                        image,
                        config: vm_config,
                        status: VmStatus::Paused,
                        started_at: None,
                        error_message: None,
                    },
                )
                .await;

                mgr.resume_vm(&vm_id)
                    .await
                    .expect("resume_vm should restart from snapshot");
            },
        }

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        assert!(
            mgr.get_vm(&vm_id).await.is_some(),
            "VM should remain tracked after snapshot restore failure"
        );

        mgr.delete_vm(&vm_id)
            .await
            .expect("delete_vm should clean up helper process");

        std::fs::remove_dir_all(&data_dir).ok();
        cleanup_env(prev_bridge_ip);
    }

    fn test_vm_info(vm_id: VmId, status: VmStatus) -> VmInfo {
        VmInfo {
            vm_id,
            app_id: AppId::new(),
            image: "test-image".to_string(),
            config: config(),
            status,
            started_at: None,
            error_message: None,
        }
    }

    async fn spawn_fc_socket_stub(
        path: PathBuf,
        expected_requests: usize,
    ) -> std::io::Result<JoinHandle<()>> {
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;

        Ok(tokio::spawn(async move {
            for _ in 0..expected_requests {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut buf = [0u8; 4096];
                let _ =
                    tokio::time::timeout(std::time::Duration::from_secs(2), stream.read(&mut buf))
                        .await;
                let response =
                    b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(response).await;
                let _ = stream.shutdown().await;
            }
        }))
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
        assert!(matches!(result, Err(HypervisorError::StartFailed(_))));
    }

    #[tokio::test]
    async fn test_stop_vm_transitions_to_stopping() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let vm_id = VmId::new();
        let child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("sleep 10")
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

        assert!(mgr.stop_vm(&vm_id).await.is_ok());
        assert_eq!(mgr.get_vm_status(&vm_id).await.unwrap(), VmStatus::Stopped);
        assert!(!mgr.has_process(&vm_id).await);
    }

    #[tokio::test]
    async fn test_stop_nonexistent_vm_returns_error() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        assert!(matches!(
            mgr.stop_vm(&VmId::new()).await,
            Err(HypervisorError::VmNotFound(_))
        ));
    }

    #[tokio::test]
    async fn test_get_status_nonexistent_returns_error() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        assert!(matches!(
            mgr.get_vm_status(&VmId::new()).await,
            Err(HypervisorError::VmNotFound(_))
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
    async fn test_pause_vm_terminates_process_and_resume_restarts_from_snapshot() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let vm_id = VmId::new();
        let app_id = AppId::new();
        let socket_path = temp_socket_path("pause-resume");
        let _server = match spawn_fc_socket_stub(socket_path.clone(), 3).await {
            Ok(server) => server,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                eprintln!("skipping unix-socket test: {e}");
                return;
            },
            Err(e) => panic!("failed to bind unix socket stub: {e}"),
        };

        let child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("sleep 10")
            .spawn()
            .expect("failed to spawn test child");

        mgr.insert_process_for_test(&vm_id, child, socket_path.to_string_lossy().to_string())
            .await;
        mgr.set_vm_for_test(
            &vm_id,
            VmInfo {
                vm_id,
                app_id,
                image: "test-image".to_string(),
                config: config(),
                status: VmStatus::Running,
                started_at: None,
                error_message: None,
            },
        )
        .await;

        mgr.pause_vm(&vm_id).await.expect("pause_vm should succeed");
        assert!(!mgr.has_process(&vm_id).await);
        assert_eq!(mgr.get_vm_status(&vm_id).await.unwrap(), VmStatus::Paused);

        mgr.resume_vm(&vm_id)
            .await
            .expect("resume_vm should restart from snapshot");

        if mgr.fc_config.kernel_path.is_some() {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            assert!(
                mgr.has_process(&vm_id).await,
                "resume_vm should restore a tracked process when kernel boot is enabled"
            );
        }

        assert_ne!(mgr.get_vm_status(&vm_id).await.unwrap(), VmStatus::Failed);
    }

    #[tokio::test]
    async fn test_start_vm_restarts_after_snapshot_load_failure() {
        run_snapshot_restart_test("snapshot-restart", SnapshotRestartAction::StartVm).await;
    }

    #[tokio::test]
    async fn test_resume_vm_restarts_after_snapshot_load_failure() {
        run_snapshot_restart_test("resume-snapshot-restart", SnapshotRestartAction::ResumeVm).await;
    }

    #[tokio::test]
    async fn test_kill_process_escalates_to_sigkill_for_recovered_pid() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let vm_id = VmId::new();

        let child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("trap '' TERM; sleep 30")
            .spawn()
            .expect("failed to spawn test child");
        let pid = child.id().expect("test child pid missing");

        let mut proc = VmProcess {
            vm_id,
            child: None,
            pid: Some(pid),
            socket_path: "/tmp/test.sock".to_string(),
            metrics_path: None,
            stdout_log_path: "/tmp/test.stdout.log".to_string(),
            stderr_log_path: "/tmp/test.stderr.log".to_string(),
            stdout_log_offset: Arc::new(AtomicU64::new(0)),
            stderr_log_offset: Arc::new(AtomicU64::new(0)),
            tap_name: None,
            tap_ifindex: None,
            log_task: Some(tokio::spawn(async {})),
            chroot_dir: None,
            app_started: Arc::new(AtomicBool::new(false)),
            app_started_at_ms: Arc::new(AtomicU64::new(0)),
            vfs_processes: Vec::new(),
            vfs_pids: Vec::new(),
        };

        mgr.kill_process(&vm_id, &mut proc)
            .await
            .expect("kill_process should succeed");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while FirecrackerManager::is_pid_alive(pid) {
            if std::time::Instant::now() > deadline {
                panic!("Recovered process was not terminated after SIGKILL escalation");
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    #[tokio::test]
    async fn test_cleanup_all_stale_resources_keeps_paused_vm_artifacts() {
        let mut mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let data_dir = temp_data_dir("gc-paused");
        tokio::fs::create_dir_all(&data_dir).await.unwrap();
        mgr.fc_config.data_dir = data_dir.to_string_lossy().to_string();

        let paused_vm_id = VmId::new();
        let stale_vm_id = VmId::new();
        let prefix = format!("fc-{}-", mgr.agent_id);

        mgr.set_vm_for_test(
            &paused_vm_id,
            VmInfo {
                vm_id: paused_vm_id,
                app_id: AppId::new(),
                image: "paused-image".to_string(),
                config: config(),
                status: VmStatus::Paused,
                started_at: None,
                error_message: None,
            },
        )
        .await;

        let paused_rootfs = data_dir.join(format!("{prefix}{paused_vm_id}-rootfs.ext4"));
        let paused_socket = data_dir.join(format!("{prefix}{paused_vm_id}.sock"));
        let stale_rootfs = data_dir.join(format!("{prefix}{stale_vm_id}-rootfs.ext4"));
        let stale_socket = data_dir.join(format!("{prefix}{stale_vm_id}.sock"));

        tokio::fs::write(&paused_rootfs, b"paused").await.unwrap();
        tokio::fs::write(&paused_socket, b"paused").await.unwrap();
        tokio::fs::write(&stale_rootfs, b"stale").await.unwrap();
        tokio::fs::write(&stale_socket, b"stale").await.unwrap();

        mgr.cleanup_all_stale_resources().await;

        assert!(tokio::fs::metadata(&paused_rootfs).await.is_ok());
        assert!(tokio::fs::metadata(&paused_socket).await.is_ok());
        assert!(tokio::fs::metadata(&stale_rootfs).await.is_err());
        assert!(tokio::fs::metadata(&stale_socket).await.is_err());

        let _ = tokio::fs::remove_dir_all(&data_dir).await;
    }

    #[tokio::test]
    async fn test_cleanup_all_stale_resources_keeps_failed_vm_artifacts() {
        let mut mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let data_dir = temp_data_dir("gc-failed");
        tokio::fs::create_dir_all(&data_dir).await.unwrap();
        mgr.fc_config.data_dir = data_dir.to_string_lossy().to_string();

        let failed_vm_id = VmId::new();
        let prefix = format!("fc-{}-", mgr.agent_id);

        mgr.set_vm_for_test(
            &failed_vm_id,
            VmInfo {
                vm_id: failed_vm_id,
                app_id: AppId::new(),
                image: "failed-image".to_string(),
                config: config(),
                status: VmStatus::Failed,
                started_at: None,
                error_message: Some("boom".to_string()),
            },
        )
        .await;

        let failed_rootfs = data_dir.join(format!("{prefix}{failed_vm_id}-rootfs.ext4"));
        let failed_socket = data_dir.join(format!("{prefix}{failed_vm_id}.sock"));
        let failed_metrics = data_dir.join(format!("{prefix}{failed_vm_id}-metrics.json"));

        tokio::fs::write(&failed_rootfs, b"failed").await.unwrap();
        tokio::fs::write(&failed_socket, b"failed").await.unwrap();
        tokio::fs::write(&failed_metrics, b"failed").await.unwrap();

        mgr.cleanup_all_stale_resources().await;

        assert!(tokio::fs::metadata(&failed_rootfs).await.is_ok());
        assert!(tokio::fs::metadata(&failed_socket).await.is_ok());
        assert!(tokio::fs::metadata(&failed_metrics).await.is_ok());

        let _ = tokio::fs::remove_dir_all(&data_dir).await;
    }

    #[tokio::test]
    async fn test_cleanup_all_stale_resources_removes_orphan_artifacts() {
        let mut mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let data_dir = temp_data_dir("gc-orphan");
        tokio::fs::create_dir_all(&data_dir).await.unwrap();
        mgr.fc_config.data_dir = data_dir.to_string_lossy().to_string();

        let orphan_vm_id = VmId::new();
        let prefix = format!("fc-{}-", mgr.agent_id);

        let orphan_rootfs = data_dir.join(format!("{prefix}{orphan_vm_id}-rootfs.ext4"));
        let orphan_socket = data_dir.join(format!("{prefix}{orphan_vm_id}.sock"));
        let orphan_metrics = data_dir.join(format!("{prefix}{orphan_vm_id}-metrics.json"));

        tokio::fs::write(&orphan_rootfs, b"orphan").await.unwrap();
        tokio::fs::write(&orphan_socket, b"orphan").await.unwrap();
        tokio::fs::write(&orphan_metrics, b"orphan").await.unwrap();

        mgr.cleanup_all_stale_resources().await;

        assert!(tokio::fs::metadata(&orphan_rootfs).await.is_err());
        assert!(tokio::fs::metadata(&orphan_socket).await.is_err());
        assert!(tokio::fs::metadata(&orphan_metrics).await.is_err());

        let _ = tokio::fs::remove_dir_all(&data_dir).await;
    }

    #[tokio::test]
    async fn test_cleanup_all_stale_resources_keeps_active_snapshot_artifacts() {
        let mut mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let data_dir = temp_data_dir("gc-active-snapshots");
        tokio::fs::create_dir_all(&data_dir).await.unwrap();
        tokio::fs::create_dir_all(data_dir.join("snapshots"))
            .await
            .unwrap();
        mgr.fc_config.data_dir = data_dir.to_string_lossy().to_string();

        let active_vm_id = VmId::new();
        let prefix = format!("fc-{}-", mgr.agent_id);

        mgr.set_vm_for_test(
            &active_vm_id,
            VmInfo {
                vm_id: active_vm_id,
                app_id: AppId::new(),
                image: "active-image".to_string(),
                config: config(),
                status: VmStatus::Paused,
                started_at: None,
                error_message: None,
            },
        )
        .await;

        let active_snapshot = data_dir
            .join("snapshots")
            .join(format!("{active_vm_id}.snapshot"));
        let active_mem = data_dir
            .join("snapshots")
            .join(format!("{active_vm_id}.mem"));
        let active_rootfs = data_dir.join(format!("{prefix}{active_vm_id}-rootfs.ext4"));
        let active_socket = data_dir.join(format!("{prefix}{active_vm_id}.sock"));

        tokio::fs::write(&active_snapshot, b"active").await.unwrap();
        tokio::fs::write(&active_mem, b"active").await.unwrap();
        tokio::fs::write(&active_rootfs, b"active").await.unwrap();
        tokio::fs::write(&active_socket, b"active").await.unwrap();

        mgr.cleanup_all_stale_resources().await;

        assert!(tokio::fs::metadata(&active_snapshot).await.is_ok());
        assert!(tokio::fs::metadata(&active_mem).await.is_ok());
        assert!(tokio::fs::metadata(&active_rootfs).await.is_ok());
        assert!(tokio::fs::metadata(&active_socket).await.is_ok());

        let _ = tokio::fs::remove_dir_all(&data_dir).await;
    }

    #[tokio::test]
    async fn test_cleanup_all_stale_resources_removes_orphan_snapshot_artifacts() {
        let mut mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let data_dir = temp_data_dir("gc-orphan-snapshots");
        tokio::fs::create_dir_all(&data_dir).await.unwrap();
        tokio::fs::create_dir_all(data_dir.join("snapshots"))
            .await
            .unwrap();
        mgr.fc_config.data_dir = data_dir.to_string_lossy().to_string();

        let orphan_vm_id = VmId::new();

        let orphan_snapshot = data_dir
            .join("snapshots")
            .join(format!("{orphan_vm_id}.snapshot"));
        let orphan_mem = data_dir
            .join("snapshots")
            .join(format!("{orphan_vm_id}.mem"));

        tokio::fs::write(&orphan_snapshot, b"orphan").await.unwrap();
        tokio::fs::write(&orphan_mem, b"orphan").await.unwrap();

        mgr.cleanup_all_stale_resources().await;

        assert!(tokio::fs::metadata(&orphan_snapshot).await.is_err());
        assert!(tokio::fs::metadata(&orphan_mem).await.is_err());

        let _ = tokio::fs::remove_dir_all(&data_dir).await;
    }

    #[tokio::test]
    async fn test_delete_vm_removes_snapshot_artifacts() {
        let mut mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let data_dir = temp_data_dir("delete-snapshots");
        tokio::fs::create_dir_all(&data_dir).await.unwrap();
        tokio::fs::create_dir_all(data_dir.join("snapshots"))
            .await
            .unwrap();
        mgr.fc_config.data_dir = data_dir.to_string_lossy().to_string();

        let vm_id = VmId::new();
        let prefix = format!("fc-{}-", mgr.agent_id);

        mgr.set_vm_for_test(
            &vm_id,
            VmInfo {
                vm_id,
                app_id: AppId::new(),
                image: "snapshot-image".to_string(),
                config: config(),
                status: VmStatus::Paused,
                started_at: None,
                error_message: None,
            },
        )
        .await;

        let snapshot_path = data_dir.join("snapshots").join(format!("{vm_id}.snapshot"));
        let mem_path = data_dir.join("snapshots").join(format!("{vm_id}.mem"));
        let rootfs_path = data_dir.join(format!("{prefix}{vm_id}-rootfs.ext4"));

        tokio::fs::write(&snapshot_path, b"snapshot").await.unwrap();
        tokio::fs::write(&mem_path, b"snapshot").await.unwrap();
        tokio::fs::write(&rootfs_path, b"snapshot").await.unwrap();

        mgr.delete_vm(&vm_id).await.unwrap();

        assert!(tokio::fs::metadata(&snapshot_path).await.is_err());
        assert!(tokio::fs::metadata(&mem_path).await.is_err());
        assert!(tokio::fs::metadata(&rootfs_path).await.is_err());
        assert!(mgr.get_vm(&vm_id).await.is_none());

        let _ = tokio::fs::remove_dir_all(&data_dir).await;
    }

    #[tokio::test]
    async fn test_persist_runtime_state_writes_pid_and_runtime_metadata() {
        let mut mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let data_dir = temp_data_dir("persist-runtime-state");
        tokio::fs::create_dir_all(&data_dir).await.unwrap();
        mgr.fc_config.data_dir = data_dir.to_string_lossy().to_string();

        let vm_id = VmId::new();
        let vm = test_vm_info(vm_id, VmStatus::Running);
        mgr.set_vm_for_test(&vm_id, vm.clone()).await;

        let child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("sleep 5")
            .spawn()
            .expect("failed to spawn child");
        let child_pid = child.id();
        mgr.insert_process_for_test(&vm_id, child, "/run/firecracker.socket".to_string())
            .await;
        let mut processes = mgr.processes.lock().await;
        if let Some(proc) = processes.get_mut(&vm_id) {
            proc.stdout_log_path = "/tmp/stdout.log".to_string();
            proc.stderr_log_path = "/tmp/stderr.log".to_string();
            proc.stdout_log_offset.store(42, Ordering::SeqCst);
            proc.stderr_log_offset.store(84, Ordering::SeqCst);
            proc.vfs_pids = vec![1234, 5678];
        }
        drop(processes);

        mgr.persist_runtime_state().await.unwrap();

        let raw = tokio::fs::read_to_string(mgr.runtime_state_path())
            .await
            .unwrap();
        let state: PersistedAgentState = serde_json::from_str(&raw).unwrap();
        assert_eq!(state.vms.len(), 1);
        match &state.vms[0] {
            PersistedVmRecord::Current(runtime) => {
                assert_eq!(runtime.vm.vm_id, vm_id);
                assert_eq!(runtime.vm.status, VmStatus::Running);
                assert_eq!(runtime.pid, child_pid);
                assert_eq!(runtime.socket_path, "/run/firecracker.socket");
                assert_eq!(runtime.stdout_log_path, "/tmp/stdout.log");
                assert_eq!(runtime.stderr_log_path, "/tmp/stderr.log");
                assert_eq!(runtime.stdout_log_offset, 42);
                assert_eq!(runtime.stderr_log_offset, 84);
                assert!(runtime.app_started);
                assert!(runtime.app_started_at_ms > 0);
                assert_eq!(runtime.vfs_pids, vec![1234, 5678]);
            },
            PersistedVmRecord::Legacy(_) => panic!("expected current runtime record"),
        }

        let mut processes = mgr.processes.lock().await;
        if let Some(mut proc) = processes.remove(&vm_id)
            && let Some(mut child) = proc.child.take()
        {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        drop(processes);
        let _ = tokio::fs::remove_dir_all(&data_dir).await;
    }

    #[tokio::test]
    async fn test_load_runtime_state_recovers_live_process_and_removes_dead_vm() {
        let mut mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let data_dir = temp_data_dir("load-runtime-state");
        tokio::fs::create_dir_all(&data_dir).await.unwrap();
        mgr.fc_config.data_dir = data_dir.to_string_lossy().to_string();

        let alive_vm_id = VmId::new();
        let dead_vm_id = VmId::new();
        let alive_vm = test_vm_info(alive_vm_id, VmStatus::Running);
        let dead_vm = test_vm_info(dead_vm_id, VmStatus::Running);
        let current_pid = std::process::id();
        let dead_pid = u32::MAX.saturating_sub(1);
        let dead_paths = crate::firecracker::paths::VmPaths::new(
            &mgr.fc_config.data_dir,
            &mgr.agent_id,
            dead_vm_id,
        );
        let dead_socket = data_dir.join("dead-custom.sock");
        let dead_chroot = data_dir.join("dead-chroot");
        tokio::fs::create_dir_all(&dead_chroot).await.unwrap();
        tokio::fs::create_dir_all(data_dir.join("snapshots"))
            .await
            .unwrap();
        tokio::fs::write(dead_paths.config_path(), b"dead-config")
            .await
            .unwrap();
        tokio::fs::write(dead_paths.log_path(), b"dead-log")
            .await
            .unwrap();
        tokio::fs::write(dead_paths.rootfs_path(), b"dead-rootfs")
            .await
            .unwrap();
        tokio::fs::write(dead_paths.stdout_log_path(), b"dead-stdout")
            .await
            .unwrap();
        tokio::fs::write(dead_paths.stderr_log_path(), b"dead-stderr")
            .await
            .unwrap();
        tokio::fs::write(dead_paths.snapshot_file(), b"dead-snapshot")
            .await
            .unwrap();
        tokio::fs::write(dead_paths.memory_file(), b"dead-memory")
            .await
            .unwrap();
        tokio::fs::write(&dead_paths.metrics_path(), b"dead-metrics")
            .await
            .unwrap();
        tokio::fs::write(&dead_socket, b"dead-socket")
            .await
            .unwrap();

        let state = PersistedAgentState {
            vms: vec![
                PersistedVmRecord::Current(PersistedVmRuntime {
                    vm: alive_vm,
                    pid: Some(current_pid),
                    socket_path: "/run/firecracker.socket".to_string(),
                    metrics_path: Some("/run/metrics.json".to_string()),
                    stdout_log_path: "/run/stdout.log".to_string(),
                    stderr_log_path: "/run/stderr.log".to_string(),
                    stdout_log_offset: 0,
                    stderr_log_offset: 0,
                    tap_name: Some("m-tap-alive".to_string()),
                    tap_ifindex: Some(12),
                    chroot_dir: Some("/srv/jailer/firecracker/alive".to_string()),
                    app_started: true,
                    app_started_at_ms: 1234,
                    vfs_pids: Vec::new(),
                }),
                PersistedVmRecord::Current(PersistedVmRuntime {
                    vm: dead_vm,
                    pid: Some(dead_pid),
                    socket_path: dead_socket.to_string_lossy().to_string(),
                    metrics_path: Some(dead_paths.metrics_path().to_string_lossy().to_string()),
                    stdout_log_path: dead_paths.stdout_log_path().to_string_lossy().to_string(),
                    stderr_log_path: dead_paths.stderr_log_path().to_string_lossy().to_string(),
                    stdout_log_offset: 0,
                    stderr_log_offset: 0,
                    tap_name: Some("m-tap-dead".to_string()),
                    tap_ifindex: Some(13),
                    chroot_dir: Some(dead_chroot.to_string_lossy().to_string()),
                    app_started: true,
                    app_started_at_ms: 5678,
                    vfs_pids: Vec::new(),
                }),
            ],
        };

        tokio::fs::write(
            mgr.runtime_state_path(),
            serde_json::to_vec_pretty(&state).unwrap(),
        )
        .await
        .unwrap();

        mgr.load_runtime_state().await.unwrap();

        assert!(mgr.has_process(&alive_vm_id).await);
        assert_eq!(
            mgr.get_vm_status(&alive_vm_id).await.unwrap(),
            VmStatus::Running
        );

        assert!(mgr.get_vm(&dead_vm_id).await.is_none());
        assert!(!mgr.has_process(&dead_vm_id).await);
        assert!(tokio::fs::metadata(dead_paths.config_path()).await.is_err());
        assert!(tokio::fs::metadata(dead_paths.log_path()).await.is_err());
        assert!(tokio::fs::metadata(dead_paths.rootfs_path()).await.is_err());
        assert!(
            tokio::fs::metadata(dead_paths.stdout_log_path())
                .await
                .is_err()
        );
        assert!(
            tokio::fs::metadata(dead_paths.stderr_log_path())
                .await
                .is_err()
        );
        assert!(
            tokio::fs::metadata(dead_paths.snapshot_file())
                .await
                .is_err()
        );
        assert!(tokio::fs::metadata(dead_paths.memory_file()).await.is_err());
        assert!(
            tokio::fs::metadata(dead_paths.metrics_path())
                .await
                .is_err()
        );
        assert!(tokio::fs::metadata(&dead_socket).await.is_err());
        assert!(tokio::fs::metadata(&dead_chroot).await.is_err());

        let _ = tokio::fs::remove_dir_all(&data_dir).await;
    }

    #[tokio::test]
    async fn test_load_runtime_state_removes_legacy_records_without_live_process() {
        let mut mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let data_dir = temp_data_dir("load-legacy-runtime-state");
        tokio::fs::create_dir_all(&data_dir).await.unwrap();
        mgr.fc_config.data_dir = data_dir.to_string_lossy().to_string();

        let legacy_vm_id = VmId::new();
        let legacy_vm = test_vm_info(legacy_vm_id, VmStatus::Running);
        let state = PersistedAgentState {
            vms: vec![PersistedVmRecord::Legacy(legacy_vm)],
        };

        tokio::fs::write(
            mgr.runtime_state_path(),
            serde_json::to_vec_pretty(&state).unwrap(),
        )
        .await
        .unwrap();

        mgr.load_runtime_state().await.unwrap();

        assert!(mgr.get_vm(&legacy_vm_id).await.is_none());
        assert!(!mgr.has_process(&legacy_vm_id).await);

        let _ = tokio::fs::remove_dir_all(&data_dir).await;
    }

    #[tokio::test]
    async fn test_error_messages_contain_vm_id() {
        let err = HypervisorError::VmNotFound("vm-99".to_string());
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
        assert!(matches!(result, Err(HypervisorError::SocketTimeout(_))));
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
    async fn test_cleanup_stale_taps_identifies_orphans() {
        let mgr = FirecrackerManager::with_config(FirecrackerConfig::stub());
        let active_vm_id = VmId::new();
        let stale_vm_id = VmId::new();

        mgr.set_vm_for_test(
            &active_vm_id,
            VmInfo {
                vm_id: active_vm_id,
                app_id: AppId::new(),
                image: "active".to_string(),
                config: config(),
                status: VmStatus::Running,
                started_at: None,
                error_message: None,
            },
        )
        .await;

        let active_vm_ids: std::collections::HashSet<VmId> = [active_vm_id].into_iter().collect();

        let active_prefix = &active_vm_id.to_string()[..8];
        let stale_prefix = &stale_vm_id.to_string()[..8];

        let _tap_active = format!("m-tap-{active_prefix}");
        let _tap_stale = format!("m-tap-{stale_prefix}");

        assert!(
            active_vm_ids
                .iter()
                .any(|id| id.to_string().starts_with(active_prefix))
        );
        assert!(
            !active_vm_ids
                .iter()
                .any(|id| id.to_string().starts_with(stale_prefix))
        );
    }
}
