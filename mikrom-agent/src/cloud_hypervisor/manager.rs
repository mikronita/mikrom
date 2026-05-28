use crate::cloud_hypervisor::api::{ch_request, wait_for_socket};
use crate::cloud_hypervisor::config as ch_config;
use crate::cloud_hypervisor::process::CloudHypervisorProcess;
use crate::config::AgentConfig;
use crate::hypervisor::vm_hypervisor::{HypervisorType, VmHypervisor};
use crate::hypervisor::{HypervisorError, VmConfig, VmDetailedInfo, VmInfo, VmStatus};
use async_trait::async_trait;
use dashmap::DashMap;
use mikrom_proto::id::{AppId, VmId};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct CloudHypervisorManager {
    pub(crate) config: AgentConfig,
    active_vms: Arc<DashMap<VmId, Arc<RwLock<CloudHypervisorProcess>>>>,
    vms: Arc<DashMap<VmId, VmInfo>>,
    builder: Arc<crate::builder::ImageBuilder>,
    nats_client: Arc<RwLock<Option<async_nats::Client>>>,
}

impl CloudHypervisorManager {
    pub async fn new(config: AgentConfig) -> Self {
        let builder = crate::builder::ImageBuilder::new().unwrap_or_else(|e| {
            tracing::error!("ImageBuilder::new failed (should never happen): {e}");
            crate::builder::ImageBuilder
        });

        Self {
            config,
            active_vms: Arc::new(DashMap::new()),
            vms: Arc::new(DashMap::new()),
            builder: Arc::new(builder),
            nats_client: Arc::new(RwLock::new(None)),
        }
    }

    fn build_boot_args(&self, _config: &VmConfig) -> String {
        "console=ttyS0 earlyprintk=ttyS0 reboot=k panic=1 net.ifnames=0 nomodules rw root=/dev/vda init=/mikrom-init"
            .to_string()
    }

    fn get_vm_paths(&self, vm_id: &VmId) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        let base = self.config.data_path.join("vms").join(vm_id.to_string());
        let _ = std::fs::create_dir_all(&base);
        (
            base.join("ch.sock"),
            base.join("ch.log"),
            base.join("ch.pid"),
            base.join("ch-serial.log"),
        )
    }

    async fn start_vm_background(
        &self,
        vm_id: VmId,
        app_id: AppId,
        image: String,
        config: VmConfig,
    ) -> Result<(), HypervisorError> {
        let (socket_path, log_path, _pid_path, serial_log) = self.get_vm_paths(&vm_id);
        let socket_str = socket_path.to_string_lossy().to_string();
        let serial_log_str = serial_log.to_string_lossy().to_string();

        // 1. Networking: Setup TAP
        let (tap_name, tap_ifindex) = self.setup_tap(&vm_id).await?;
        self.setup_routing(config.ipv6_address.as_deref()).await?;

        // 2. Storage: Ensure volumes
        let mut disks = Vec::new();
        let mut fs_devices = Vec::new();
        let mut sidecars = Vec::new();

        let rootfs_path = self
            .config
            .data_path
            .join("vms")
            .join(vm_id.to_string())
            .join("rootfs.ext4");

        tracing::info!(vm_id = %vm_id, image = %image, "Preparing rootfs for Cloud Hypervisor");
        self.builder
            .docker_to_ext4(crate::builder::DockerToExt4Params {
                image: &image,
                output_path: &rootfs_path,
                base_rootfs_path: &self.config.cloud_hypervisor_base_rootfs.to_string_lossy(),
                port: config.port,
                ipv6_addr: config.ipv6_address.clone(),
                ipv6_gw: config.ipv6_gateway.clone(),
                volumes: &config.volumes,
            })
            .await
            .map_err(|e| HypervisorError::ProcessError(format!("Image builder failed: {e}")))?;

        disks.push(ch_config::DiskConfig {
            path: rootfs_path.to_string_lossy().to_string(),
            readonly: Some(false),
            image_type: Some("Raw".to_string()),
        });

        for vol in &config.volumes {
            let host_path = self.ensure_volume(vol).await?;
            use mikrom_proto::agent::AccessMode;
            if vol.access_mode == AccessMode::ReadWriteMany as i32 {
                let fs_socket = self
                    .config
                    .data_path
                    .join("vms")
                    .join(vm_id.to_string())
                    .join(format!("fs-{}.sock", vol.volume_id));
                let sidecar = self
                    .start_virtiofsd(&vm_id, &vol.volume_id, &host_path, &fs_socket)
                    .await?;
                sidecars.push(sidecar);

                fs_devices.push(ch_config::FsConfig {
                    tag: vol.volume_id.clone(),
                    socket: fs_socket.to_string_lossy().to_string(),
                    num_queues: 1,
                    queue_size: 1024,
                });
            } else {
                disks.push(ch_config::DiskConfig {
                    path: host_path,
                    readonly: Some(vol.read_only),
                    image_type: Some("Raw".to_string()),
                });
            }
        }

        // 3. Spawn CH process
        let mut proc = CloudHypervisorProcess::spawn(
            &self.config.cloud_hypervisor_binary,
            vm_id.to_string(),
            socket_path.clone(),
            log_path,
            Some(tap_ifindex),
        )
        .await?;
        proc.sidecars = sidecars;

        // 4. Wait for API socket
        wait_for_socket(&socket_str, std::time::Duration::from_secs(10)).await?;

        // 5. Assemble CH VmConfig
        let mut ch_vm_config = ch_config::VmConfig {
            cpus: ch_config::CpusConfig {
                boot_vcpus: config.vcpus,
                max_vcpus: config.vcpus,
            },
            memory: ch_config::MemoryConfig {
                size: config.memory_mib * 1024 * 1024,
            },
            payload: ch_config::PayloadConfig {
                kernel: self
                    .config
                    .cloud_hypervisor_kernel
                    .to_string_lossy()
                    .to_string(),
                cmdline: Some(self.build_boot_args(&config)),
                ..Default::default()
            },
            console: Some(ch_config::ConsoleConfig {
                mode: "Off".to_string(),
                ..Default::default()
            }),
            serial: Some(ch_config::SerialConfig {
                mode: "File".to_string(),
                file: Some(serial_log_str),
                ..Default::default()
            }),
            disks: Some(disks),
            net: Some(vec![ch_config::NetConfig {
                id: Some("net0".to_string()),
                tap: tap_name,
                num_queues: Some(2),
                offload_tso: Some(false),
                offload_ufo: Some(false),
                offload_csum: Some(false),
                ..Default::default()
            }]),
            rng: Some(ch_config::RngConfig {
                src: "/dev/urandom".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        };

        if !fs_devices.is_empty() {
            ch_vm_config.fs = Some(fs_devices);
        }

        let body = serde_json::to_string(&ch_vm_config).map_err(|e| HypervisorError::ApiError {
            path: "/api/v1/vm.create".to_string(),
            msg: format!("Failed to serialize VmConfig: {e}"),
        })?;

        // 6. Send vm.create
        match ch_request("PUT", &socket_str, "/api/v1/vm.create", Some(&body)).await {
            Ok(_) => tracing::info!(vm_id = %vm_id, "Cloud Hypervisor VM created successfully"),
            Err(e) => {
                // If it timed out, verify if it was actually created
                if e.to_string().contains("timeout") {
                    tracing::warn!(vm_id = %vm_id, "vm.create timed out, verifying current state...");
                    let info = ch_request("GET", &socket_str, "/api/v1/vm.info", None).await?;
                    if info.contains("\"Created\"") || info.contains("\"Running\"") {
                        tracing::info!(vm_id = %vm_id, "Verified VM exists despite timeout");
                    } else {
                        return Err(e);
                    }
                } else {
                    return Err(e);
                }
            },
        }

        // 7. Send vm.boot
        match ch_request("PUT", &socket_str, "/api/v1/vm.boot", None).await {
            Ok(_) => tracing::info!(vm_id = %vm_id, "Cloud Hypervisor VM booted successfully"),
            Err(e) => {
                if e.to_string().contains("timeout") {
                    tracing::warn!(vm_id = %vm_id, "vm.boot timed out, verifying current state...");
                    let info = ch_request("GET", &socket_str, "/api/v1/vm.info", None).await?;
                    if info.contains("\"Running\"") {
                        tracing::info!(vm_id = %vm_id, "Verified VM is Running despite boot timeout");
                    } else {
                        return Err(e);
                    }
                } else {
                    return Err(e);
                }
            },
        }

        let proc_arc = Arc::new(RwLock::new(proc));
        self.active_vms.insert(vm_id, proc_arc.clone());
        self.vms.insert(
            vm_id,
            VmInfo {
                vm_id,
                app_id,
                image,
                config,
                status: VmStatus::Running,
                started_at: Some(chrono::Utc::now().timestamp_millis()),
                error_message: None,
            },
        );

        // 8. Persist state for reconciliation
        let proc_lock = proc_arc.read().await;
        if let Err(e) = self.persist_vm_state(&vm_id, &proc_lock).await {
            tracing::warn!(vm_id = %vm_id, "Failed to persist VM state: {e}");
        }

        Ok(())
    }

    fn clone_stub(&self) -> Self {
        Self {
            config: self.config.clone(),
            active_vms: self.active_vms.clone(),
            vms: self.vms.clone(),
            builder: self.builder.clone(),
            nats_client: self.nats_client.clone(),
        }
    }

    async fn run_gc(&self) {
        let mut exited = Vec::new();
        for entry in self.active_vms.iter_mut() {
            let vm_id = *entry.key();
            let proc_lock = entry.value();
            let mut proc = proc_lock.write().await;

            match proc.child.try_wait() {
                Ok(Some(status)) => {
                    tracing::info!(vm_id = %vm_id, status = ?status, "Detected Cloud Hypervisor process exit via GC");
                    exited.push(vm_id);
                },
                Ok(None) => {},
                Err(e) => {
                    tracing::error!(vm_id = %vm_id, error = %e, "Error checking Cloud Hypervisor process status");
                },
            }
        }

        for vm_id in exited {
            if let Some((_, proc_lock)) = self.active_vms.remove(&vm_id) {
                let mut proc = proc_lock.write().await;
                let _ = proc.kill().await;
                let tap_name = format!("ch-tap-{}", &vm_id.to_string()[..8]);
                self.cleanup_tap(&tap_name).await;
            }
            self.vms.remove(&vm_id);
            let path = self.config.data_path.join("vms").join(vm_id.to_string());
            let _ = tokio::fs::remove_dir_all(path).await;
        }
    }

    async fn persist_vm_state(
        &self,
        vm_id: &VmId,
        proc: &CloudHypervisorProcess,
    ) -> Result<(), HypervisorError> {
        let info = self.vms.get(vm_id).ok_or_else(|| {
            HypervisorError::ProcessError(format!("VM info not found for {vm_id}"))
        })?;

        let state = PersistedChVm {
            vm_id: *vm_id,
            app_id: info.app_id,
            pid: proc.pid,
            ipv6: info.config.ipv6_address.clone(),
            gw6: info.config.ipv6_gateway.clone(),
            mac: info.config.mac_address.clone(),
            tap_name: format!("ch-tap-{}", &vm_id.to_string()[..8]),
            tap_ifindex: proc.tap_ifindex,
            started_at: info.started_at.unwrap_or(0),
        };

        let path = self
            .config
            .data_path
            .join("vms")
            .join(vm_id.to_string())
            .join("state.json");
        let payload = serde_json::to_vec_pretty(&state).map_err(|e| {
            HypervisorError::ProcessError(format!("Failed to serialize VM state: {e}"))
        })?;

        tokio::fs::write(path, payload)
            .await
            .map_err(|e| HypervisorError::ProcessError(format!("Failed to write VM state: {e}")))?;

        Ok(())
    }

    fn is_pid_alive(&self, pid: u32) -> bool {
        let mut system = sysinfo::System::new();
        system.refresh_processes(
            sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from(pid as usize)]),
            true,
        );
        if let Some(process) = system.process(sysinfo::Pid::from(pid as usize)) {
            process.name().to_string_lossy().contains("cloud-hypervis")
        } else {
            false
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedChVm {
    vm_id: VmId,
    app_id: AppId,
    pid: u32,
    ipv6: Option<String>,
    gw6: Option<String>,
    mac: Option<String>,
    tap_name: String,
    tap_ifindex: Option<u32>,
    started_at: i64,
}

impl fmt::Debug for CloudHypervisorManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloudHypervisorManager").finish()
    }
}

#[async_trait]
impl VmHypervisor for CloudHypervisorManager {
    fn hypervisor_type(&self) -> HypervisorType {
        HypervisorType::CloudHypervisor
    }

    fn agent_id(&self) -> &str {
        &self.config.host_id
    }

    async fn start_vm(
        &self,
        vm_id: VmId,
        app_id: AppId,
        image: String,
        config: VmConfig,
    ) -> Result<(), HypervisorError> {
        let self_clone = self.clone_stub();
        let image_clone = image.clone();
        let config_clone = config.clone();
        tokio::spawn(async move {
            if let Err(e) = self_clone
                .start_vm_background(vm_id, app_id, image, config)
                .await
            {
                tracing::error!(vm_id = %vm_id, error = %e, "Failed to start Cloud Hypervisor VM in background");
                self_clone.vms.insert(
                    vm_id,
                    VmInfo {
                        vm_id,
                        app_id,
                        image: image_clone,
                        config: config_clone,
                        status: VmStatus::Failed,
                        started_at: None,
                        error_message: Some(e.to_string()),
                    },
                );
            }
        });
        Ok(())
    }

    async fn stop_vm(&self, vm_id: &VmId) -> Result<(), HypervisorError> {
        let (socket_path, _, _, _) = self.get_vm_paths(vm_id);
        let socket_str = socket_path.to_string_lossy().to_string();

        if let Some((_, proc_lock)) = self.active_vms.remove(vm_id) {
            let mut proc = proc_lock.write().await;

            // 1. Try graceful shutdown via ACPI power button
            tracing::info!(vm_id = %vm_id, "Attempting graceful shutdown via power button...");
            let _ = ch_request("PUT", &socket_str, "/api/v1/vm.power-button", None).await;

            // 2. Wait for exit
            let mut exited = false;
            for _ in 0..50 {
                match proc.child.try_wait() {
                    Ok(Some(_)) => {
                        exited = true;
                        break;
                    },
                    _ => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
                }
            }

            // 3. Fallback to hard kill if still running
            if !exited {
                tracing::warn!(vm_id = %vm_id, "Graceful shutdown timed out, performing hard kill");
                let _ = proc.kill().await;
            }

            let tap_name = format!("ch-tap-{}", &vm_id.to_string()[..8]);
            self.cleanup_tap(&tap_name).await;
        }
        self.vms.remove(vm_id);

        // Remove state file
        let path = self.config.data_path.join("vms").join(vm_id.to_string());
        let _ = tokio::fs::remove_dir_all(path).await;

        Ok(())
    }

    async fn pause_vm(&self, _vm_id: &VmId) -> Result<(), HypervisorError> {
        Err(HypervisorError::UnsupportedOperation("pause".to_string()))
    }

    async fn resume_vm(&self, _vm_id: &VmId) -> Result<(), HypervisorError> {
        Err(HypervisorError::UnsupportedOperation("resume".to_string()))
    }

    async fn delete_vm(&self, vm_id: &VmId) -> Result<(), HypervisorError> {
        // 1. Best-effort stop
        let _ = self.stop_vm(vm_id).await;

        let (socket_path, _, _, _) = self.get_vm_paths(vm_id);

        if socket_path.exists() {
            tracing::info!(vm_id = %vm_id, "Attempting to shut down orphaned Cloud Hypervisor via API");
            let _ = ch_request(
                "PUT",
                &socket_path.to_string_lossy(),
                "/api/v1/vm.power-button",
                None,
            )
            .await;
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let _ = ch_request(
                "PUT",
                &socket_path.to_string_lossy(),
                "/api/v1/vm.shutdown",
                None,
            )
            .await;
        }

        let path = self.config.data_path.join("vms").join(vm_id.to_string());
        let state_path = path.join("state.json");

        if state_path.exists()
            && let Ok(state_str) = tokio::fs::read_to_string(&state_path).await
            && let Ok(state) = serde_json::from_str::<PersistedChVm>(&state_str)
        {
            let pid = state.pid;
            if self.is_pid_alive(pid) {
                tracing::info!(vm_id = %vm_id, pid = pid, "Killing orphaned Cloud Hypervisor process via PID");
                let _ = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let _ = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
            }
        }

        let tap_name = format!("ch-tap-{}", &vm_id.to_string()[..8]);
        self.cleanup_tap(&tap_name).await;

        // 2. Remove state directory
        let _ = tokio::fs::remove_dir_all(path).await;

        // 3. Finally remove from memory
        self.vms.remove(vm_id);

        Ok(())
    }

    async fn restart_vm(&self, _vm_id: &VmId) -> Result<(), HypervisorError> {
        Err(HypervisorError::UnsupportedOperation("restart".to_string()))
    }

    async fn get_vm_info(&self, vm_id: &VmId) -> Option<VmInfo> {
        let mut info = self.vms.get(vm_id)?.clone();

        let socket_path = self.get_vm_paths(vm_id).0;
        let socket_str = socket_path.to_string_lossy().to_string();

        if let Ok(resp_body) = ch_request("GET", &socket_str, "/api/v1/vm.info", None).await
            && let Ok(ch_info) = serde_json::from_str::<ch_config::VmInfoResponse>(&resp_body)
        {
            info.status = match ch_info.state.as_str() {
                "Created" => VmStatus::Starting,
                "Running" => VmStatus::Running,
                "Paused" => VmStatus::Paused,
                "Shutdown" => VmStatus::Stopped,
                _ => VmStatus::Failed,
            };
        }

        Some(info)
    }

    async fn get_all_vms(&self) -> Vec<VmDetailedInfo> {
        let mut results = Vec::new();
        for entry in self.vms.iter() {
            let vm_id = entry.key();

            if let Some(info) = self.get_vm_info(vm_id).await {
                let (pid, tap_ifindex) = if let Some(proc_lock) = self.active_vms.get(vm_id) {
                    let proc = proc_lock.read().await;
                    (Some(proc.pid), proc.tap_ifindex)
                } else {
                    (None, None)
                };

                // Fetch CH counters
                let socket_path = self.get_vm_paths(vm_id).0;
                let socket_str = socket_path.to_string_lossy().to_string();
                let raw_metrics = if info.status == VmStatus::Running {
                    match ch_request("GET", &socket_str, "/api/v1/vm.counters", None).await {
                        Ok(resp) => serde_json::from_str(&resp).ok(),
                        Err(_) => None,
                    }
                } else {
                    None
                };

                results.push(VmDetailedInfo {
                    vm_id: *vm_id,
                    app_id: info.app_id,
                    status: info.status,
                    error_message: info.error_message.clone(),
                    pid,
                    metrics_path: None,
                    socket_path: Some(socket_str),
                    tap_name: Some(format!("ch-tap-{}", &vm_id.to_string()[..8])),
                    tap_ifindex,
                    raw_metrics,
                });
            }
        }
        results
    }

    async fn get_vm_started_at_ms(&self, vm_id: &VmId) -> Option<u64> {
        self.vms
            .get(vm_id)
            .and_then(|v| v.started_at.map(|t| t as u64))
    }

    async fn is_app_started(&self, _vm_id: &VmId) -> bool {
        true // Cloud Hypervisor doesn't have a built-in health check for apps yet
    }

    fn get_logs(&self, _vm_id: &VmId) -> Vec<String> {
        Vec::new()
    }

    async fn update_vm_firewall(
        &self,
        _vm_id: &VmId,
        _rules: Vec<mikrom_agent_ebpf_common::FirewallRule>,
    ) -> Result<(), HypervisorError> {
        Ok(())
    }

    async fn init_network(&self) -> Result<(), HypervisorError> {
        Ok(())
    }

    async fn load_runtime_state(&self) -> Result<(), HypervisorError> {
        let vms_dir = self.config.data_path.join("vms");
        if !vms_dir.exists() {
            return Ok(());
        }

        let mut entries = tokio::fs::read_dir(&vms_dir)
            .await
            .map_err(|e| HypervisorError::ProcessError(format!("Failed to read vms dir: {e}")))?;

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let state_path = path.join("state.json");
            if !state_path.exists() {
                continue;
            }

            let Ok(raw) = tokio::fs::read_to_string(&state_path).await else {
                continue;
            };

            let Ok(state) = serde_json::from_str::<PersistedChVm>(&raw) else {
                continue;
            };

            // Check if PID is still alive and is cloud-hypervisor
            if self.is_pid_alive(state.pid) {
                tracing::info!(vm_id = %state.vm_id, pid = %state.pid, "Reconciling Cloud Hypervisor VM");

                // Reconstruct the process handle
                // We use a command that exits immediately just to satisfy the tokio::process::Child requirement
                let mut cmd = tokio::process::Command::new("true");
                let child = cmd.spawn().unwrap();

                let proc = CloudHypervisorProcess {
                    vm_id: state.vm_id.to_string(),
                    socket_path: self.get_vm_paths(&state.vm_id).0,
                    pid: state.pid,
                    child,
                    sidecars: Vec::new(),
                    tap_ifindex: state.tap_ifindex,
                };

                self.active_vms
                    .insert(state.vm_id, Arc::new(RwLock::new(proc)));
                self.vms.insert(
                    state.vm_id,
                    VmInfo {
                        vm_id: state.vm_id,
                        app_id: state.app_id,
                        image: "".to_string(), // Placeholder, might want to persist image too
                        config: VmConfig {
                            vcpus: 1, // Default or persist it
                            memory_mib: 512,
                            ipv6_address: state.ipv6.clone(),
                            ipv6_gateway: state.gw6.clone(),
                            mac_address: state.mac.clone(),
                            ..Default::default()
                        },
                        status: VmStatus::Running,
                        error_message: None,
                        started_at: Some(state.started_at),
                    },
                );

                // Ensure routing is still there
                let _ = self.setup_routing(state.ipv6.as_deref()).await;
            }
        }

        Ok(())
    }

    async fn persist_runtime_state(&self) -> Result<(), HypervisorError> {
        Ok(())
    }

    async fn cleanup_all_stale_resources(&self) {
        let vms_dir = self.config.data_path.join("vms");
        let Ok(mut entries) = tokio::fs::read_dir(&vms_dir).await else {
            return;
        };

        let active_ids: std::collections::HashSet<String> = self
            .active_vms
            .iter()
            .map(|e| e.key().to_string())
            .collect();

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            if let Some(vm_id_str) = path.file_name().and_then(|n| n.to_str())
                && !active_ids.contains(vm_id_str)
                && uuid::Uuid::parse_str(vm_id_str).is_ok()
            {
                tracing::debug!(vm_id = %vm_id_str, "Removing stale Cloud Hypervisor VM directory");
                let _ = tokio::fs::remove_dir_all(&path).await;
            }
        }
    }

    async fn set_nats_client(&self, client: async_nats::Client) {
        let mut nats = self.nats_client.write().await;
        *nats = Some(client);
    }

    fn start_background_tasks(&self) {
        let self_clone = Arc::new(RwLock::new(self.clone_stub()));
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
            loop {
                interval.tick().await;
                let mgr = self_clone.read().await;
                mgr.run_gc().await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_persisted_ch_vm_serialization() {
        let vm_id = VmId::new();
        let app_id = AppId::new();
        let state = PersistedChVm {
            vm_id,
            app_id,
            pid: 1234,
            ipv6: Some("fd40::1".to_string()),
            gw6: Some("fe80::1".to_string()),
            mac: Some("aa:bb:cc:dd:ee:ff".to_string()),
            tap_name: "ch-tap-test".to_string(),
            tap_ifindex: Some(1),
            started_at: 1716900000,
        };

        let json = serde_json::to_string(&state).unwrap();
        let decoded: PersistedChVm = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.vm_id, vm_id);
        assert_eq!(decoded.app_id, app_id);
        assert_eq!(decoded.pid, 1234);
        assert_eq!(decoded.ipv6, Some("fd40::1".to_string()));
        assert_eq!(decoded.gw6, Some("fe80::1".to_string()));
    }
}
