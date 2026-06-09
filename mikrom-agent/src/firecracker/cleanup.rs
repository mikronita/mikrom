use crate::hypervisor::{VmStatus, Volume};
use futures::stream::TryStreamExt;
use mikrom_proto::id::VmId;
use std::collections::HashMap;
use std::os::unix::process::ExitStatusExt;

impl crate::firecracker::FirecrackerManager {
    pub(crate) async fn run_gc(&self) {
        tracing::debug!("Running agent garbage collector...");

        #[derive(Clone)]
        enum CleanupKind {
            Paused {
                socket_path: String,
                chroot_dir: Option<String>,
            },
            Active {
                volumes: Vec<Volume>,
            },
        }

        // Step 1: Collect exited processes while holding the processes lock.
        let mut exited = Vec::new();
        {
            let mut processes = self.processes.lock().await;
            let mut to_remove = Vec::new();

            for (vm_id, proc) in processes.iter_mut() {
                let status = if let Some(child) = proc.child.as_mut() {
                    match child.try_wait() {
                        Ok(Some(status)) => Some(status),
                        Ok(None) => None,
                        Err(e) => {
                            tracing::error!(vm_id = %vm_id, error = %e, "Error checking Firecracker process status");
                            None
                        },
                    }
                } else if let Some(pid) = proc.pid {
                    if Self::is_pid_alive(pid) {
                        None
                    } else {
                        Some(std::process::ExitStatus::from_raw(0))
                    }
                } else {
                    Some(std::process::ExitStatus::from_raw(0))
                };

                if let Some(status) = status {
                    tracing::info!(vm_id = %vm_id, status = ?status, "Detected Firecracker process exit via GC");
                    to_remove.push((*vm_id, status));
                }
            }

            for (vm_id, exit_status) in to_remove {
                if let Some(proc) = processes.remove(&vm_id) {
                    exited.push((vm_id, exit_status, proc));
                }
            }
        } // processes lock dropped here

        // Step 2: Read VM statuses and decide actions while holding vms lock.
        let mut restarts = Vec::new();
        let mut cleanup_plans: HashMap<VmId, CleanupKind> = HashMap::new();
        {
            let mut vms = self.vms.write().await;
            for (vm_id, exit_status, _proc) in &exited {
                let Some(vm) = vms.get_mut(vm_id) else {
                    continue;
                };

                if vm.status == VmStatus::Running || vm.status == VmStatus::Starting {
                    tracing::error!(
                        vm_id = %vm_id,
                        exit_code = ?exit_status.code(),
                        signal = ?exit_status.signal(),
                        "VM process exited unexpectedly, preparing for auto-restart"
                    );
                    if let Some(ip) = &vm.config.ip_address {
                        self.release_vm_ip(ip).await;
                    }
                    cleanup_plans.insert(
                        *vm_id,
                        CleanupKind::Active {
                            volumes: vm.config.volumes.clone(),
                        },
                    );
                    restarts.push((*vm_id, vm.app_id, vm.image.clone(), vm.config.clone()));
                } else if vm.status == VmStatus::Paused {
                    tracing::info!(vm_id = %vm_id, "VM is hibernated, preserving artifacts");
                    cleanup_plans.insert(
                        *vm_id,
                        CleanupKind::Paused {
                            socket_path: _proc.socket_path.clone(),
                            chroot_dir: _proc.chroot_dir.clone(),
                        },
                    );
                } else {
                    tracing::info!(vm_id = %vm_id, status = ?vm.status, "VM not running, marking stopped");
                    vm.status = VmStatus::Stopped;
                    cleanup_plans.insert(
                        *vm_id,
                        CleanupKind::Active {
                            volumes: vm.config.volumes.clone(),
                        },
                    );
                }
            }
        } // vms lock dropped here

        // Step 3: Cleanup artifacts (no locks held).
        for (vm_id, _exit_status, proc) in &exited {
            let Some(kind) = cleanup_plans.get(vm_id) else {
                continue;
            };

            match kind {
                CleanupKind::Paused {
                    socket_path,
                    chroot_dir,
                } => {
                    self.cleanup_process_chroot(vm_id, chroot_dir.as_deref())
                        .await;
                    self.remove_stale_socket(&socket_path).await;
                },
                CleanupKind::Active { volumes } => {
                    self.cleanup_exited_process_artifacts(vm_id, proc, volumes)
                        .await;
                },
            }
        }

        // Step 4: Persist state and spawn restarts.
        let _ = self.persist_runtime_state().await;

        for (vid, aid, img, cfg) in restarts {
            let self_clone = self.clone();
            tokio::spawn(async move {
                tracing::info!(vm_id = %vid, "Executing auto-restart after unexpected exit");
                if let Err(e) = self_clone.start_vm(vid, aid, img, cfg).await {
                    tracing::error!(error = %e, "Auto-restart failed");
                }
            });
        }

        self.cleanup_all_stale_resources().await;
    }

    pub(crate) async fn cleanup_exited_process_artifacts(
        &self,
        vm_id: &VmId,
        proc: &crate::firecracker::process::VmProcess,
        volumes: &[Volume],
    ) {
        self.cleanup_process_paths(vm_id, Some(&proc.socket_path))
            .await;
        self.cleanup_process_chroot(vm_id, proc.chroot_dir.as_deref())
            .await;
        self.cleanup_process_volumes(volumes).await;
    }

    pub(crate) async fn cleanup_recovered_runtime_artifacts(
        &self,
        vm_id: &VmId,
        runtime: Option<&crate::firecracker::state::PersistedVmRuntime>,
        vm: &crate::hypervisor::VmInfo,
    ) {
        let socket_path = runtime.map(|r| r.socket_path.as_str());
        self.cleanup_process_paths(vm_id, socket_path).await;

        let metrics_path = runtime
            .and_then(|r| r.metrics_path.as_deref())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                crate::firecracker::paths::VmPaths::new(
                    &self.fc_config.data_dir,
                    &self.agent_id,
                    *vm_id,
                )
                .metrics_path()
            });
        if let Err(e) = tokio::fs::remove_file(&metrics_path).await
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::debug!("Failed to remove metrics file {:?}: {}", metrics_path, e);
        }

        if let Some(chroot_dir) = runtime.and_then(|r| r.chroot_dir.as_deref()) {
            self.cleanup_process_chroot(vm_id, Some(chroot_dir)).await;
        } else if self.fc_config.use_jailer {
            self.cleanup_vm_chroot(vm_id).await;
        }

        self.cleanup_process_volumes(&vm.config.volumes).await;
        self.cleanup_snapshot_files(vm_id).await;

        if let Some(tap_name) = runtime.and_then(|r| r.tap_name.as_deref()) {
            self.cleanup_tap(tap_name).await;
        }
    }

    pub(crate) async fn cleanup_process_paths(&self, vm_id: &VmId, socket_path: Option<&str>) {
        if let Some(socket) = socket_path
            && let Err(e) = tokio::fs::remove_file(socket).await
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::debug!("Failed to remove socket {}: {}", socket, e);
        }

        let paths = crate::firecracker::paths::VmPaths::new(
            &self.fc_config.data_dir,
            &self.agent_id,
            *vm_id,
        );
        for path in [paths.config_path(), paths.log_path()] {
            if let Err(e) = tokio::fs::remove_file(&path).await
                && e.kind() != std::io::ErrorKind::NotFound
            {
                tracing::debug!("Failed to remove artifact {:?}: {}", path, e);
            }
        }
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

        for log_path in [paths.stdout_log_path(), paths.stderr_log_path()] {
            if let Err(e) = tokio::fs::remove_file(&log_path).await
                && e.kind() != std::io::ErrorKind::NotFound
            {
                tracing::debug!("Failed to remove log file {:?}: {}", log_path, e);
            }
        }
    }

    pub(crate) async fn cleanup_snapshot_files(&self, vm_id: &VmId) {
        let snapshot_dir = std::path::Path::new(&self.fc_config.data_dir).join("snapshots");
        let snapshot_path = snapshot_dir.join(format!("{vm_id}.snapshot"));
        let mem_path = snapshot_dir.join(format!("{vm_id}.mem"));

        for path in [snapshot_path, mem_path] {
            if let Err(e) = tokio::fs::remove_file(&path).await
                && e.kind() != std::io::ErrorKind::NotFound
            {
                tracing::debug!("Failed to remove snapshot artifact {:?}: {}", path, e);
            }
        }
    }

    pub(crate) async fn cleanup_process_chroot(&self, vm_id: &VmId, chroot_dir: Option<&str>) {
        if let Some(chroot) = chroot_dir {
            tracing::info!(vm_id = %vm_id, chroot_dir = %chroot, "Cleaning up jailer chroot");
            if let Err(e) = tokio::fs::remove_dir_all(chroot).await {
                tracing::error!("Failed to remove chroot directory {}: {}", chroot, e);
            }
        }
    }

    /// Best-effort cleanup of the jailer chroot directory for a VM, even when
    /// the process record is no longer available.
    pub(crate) async fn cleanup_vm_chroot(&self, vm_id: &VmId) {
        let chroot_dir = self.get_chroot_dir(vm_id);
        if chroot_dir.exists() {
            tracing::info!(chroot_dir = ?chroot_dir, "Removing jailer chroot directory");
            if let Err(e) = tokio::fs::remove_dir_all(&chroot_dir).await {
                tracing::error!("Failed to remove chroot directory {:?}: {}", chroot_dir, e);
            }
        }
    }

    pub(crate) async fn cleanup_process_volumes(&self, volumes: &[Volume]) {
        let _storage = crate::ceph::CephRbd;
        for vol in volumes {
            if !vol.pool_name.is_empty() {
                let spec = format!("{}/{}", vol.pool_name, vol.volume_id);
                if let Err(e) = crate::ceph::CephRbd::unmap_volume(&spec).await {
                    tracing::warn!("Failed to unmap volume {}: {}", spec, e);
                }
            }
        }
    }

    pub(crate) async fn cleanup_all_stale_resources(&self) {
        tracing::debug!(
            agent_id = %self.agent_id,
            data_dir = %self.fc_config.data_dir,
            "Cleaning up stale Firecracker resources..."
        );
        let prefix = format!("fc-{}-", self.agent_id);

        // Keep every VM still present in memory, regardless of status.
        // Cleanup should remove only true filesystem orphans, not artifacts
        // that belong to VMs the agent still knows about after a restart.
        let active_vm_ids: std::collections::HashSet<VmId> = {
            let processes = self.processes.lock().await;
            let vms = self.vms.read().await;
            let mut ids: std::collections::HashSet<VmId> = processes.keys().cloned().collect();
            ids.extend(vms.keys().copied());
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
                            || file_name.ends_with("-metrics.json")
                            || file_name.ends_with(".stdout.log")
                            || file_name.ends_with(".stderr.log"))
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

        let snapshot_dir = std::path::Path::new(&self.fc_config.data_dir).join("snapshots");
        if let Ok(mut entries) = tokio::fs::read_dir(&snapshot_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Ok(file_name) = entry.file_name().into_string()
                    && (file_name.ends_with(".snapshot") || file_name.ends_with(".mem"))
                {
                    let vm_id_str = file_name
                        .strip_suffix(".snapshot")
                        .or_else(|| file_name.strip_suffix(".mem"));
                    if let Some(vm_id_str) = vm_id_str
                        && let Ok(vm_id) = vm_id_str.parse::<VmId>()
                        && !active_vm_ids.contains(&vm_id)
                    {
                        let path = entry.path();
                        tracing::debug!("Removing stale snapshot artifact: {:?}", path);
                        if let Err(e) = tokio::fs::remove_file(&path).await
                            && e.kind() != std::io::ErrorKind::NotFound
                        {
                            tracing::debug!(
                                "Failed to remove stale snapshot artifact {:?}: {}",
                                path,
                                e
                            );
                        }
                    }
                }
            }
        }

        self.cleanup_stale_taps(&active_vm_ids).await;
    }

    pub(crate) async fn cleanup_stale_taps(&self, active_vm_ids: &std::collections::HashSet<VmId>) {
        let handle = match self.rtnl_handle().await {
            Ok(h) => h,
            Err(_) => return,
        };

        let active_prefixes: std::collections::HashSet<String> = active_vm_ids
            .iter()
            .map(|vm_id| {
                let s = vm_id.to_string();
                if s.len() >= 8 { s[..8].to_string() } else { s }
            })
            .collect();

        let mut links = handle.link().get().execute();
        while let Ok(Some(link)) = links.try_next().await {
            let name = link.attributes.iter().find_map(|attr| match attr {
                netlink_packet_route::link::LinkAttribute::IfName(n) => Some(n.clone()),
                _ => None,
            });

            if let Some(tap_name) = name
                && tap_name.starts_with("m-tap-")
            {
                let vm_id_prefix = tap_name.strip_prefix("m-tap-").unwrap_or("");
                if vm_id_prefix.len() < 8 {
                    continue;
                }

                if !active_prefixes.contains(vm_id_prefix) {
                    tracing::info!(tap = %tap_name, "Cleaning up stale TAP interface");
                    self.cleanup_tap(&tap_name).await;
                }
            }
        }
    }

    pub(crate) fn is_active_resource_name(
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

    pub(crate) async fn remove_stale_socket<P: AsRef<std::path::Path>>(&self, socket_path: P) {
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
}
