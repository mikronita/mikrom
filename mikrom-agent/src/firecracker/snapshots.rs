use crate::firecracker::api::{fc_patch_with_timeouts, fc_put_with_timeouts};
use crate::hypervisor::{HypervisorError, VmStatus};
use mikrom_proto::id::VmId;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SnapshotRestoreOutcome {
    Restored,
    Missing,
    Failed,
}

impl crate::firecracker::FirecrackerManager {
    pub(crate) fn snapshot_create_body(snapshot_path: &str, mem_path: &str) -> String {
        serde_json::json!({
            "snapshot_type": "Full",
            "snapshot_path": snapshot_path,
            "mem_file_path": mem_path,
        })
        .to_string()
    }

    pub(crate) fn snapshot_paths(
        &self,
        vm_id: &VmId,
        chroot_dir: Option<&str>,
    ) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        let snapshot_dir = std::path::Path::new(&self.fc_config.data_dir).join("snapshots");
        let host_snapshot_path = snapshot_dir.join(format!("{vm_id}.snapshot"));
        let host_mem_path = snapshot_dir.join(format!("{vm_id}.mem"));

        match chroot_dir {
            Some(_) => (
                host_snapshot_path,
                host_mem_path,
                PathBuf::from("/vm.snapshot"),
                PathBuf::from("/vm.mem"),
            ),
            None => (
                host_snapshot_path.clone(),
                host_mem_path.clone(),
                host_snapshot_path,
                host_mem_path,
            ),
        }
    }

    pub(crate) async fn try_restore_snapshot(
        &self,
        vm_id: &VmId,
        chroot_dir: &Option<String>,
        active_socket_path: &str,
        paths: &crate::firecracker::paths::VmPaths,
    ) -> Result<SnapshotRestoreOutcome, HypervisorError> {
        let snapshot_path = paths.snapshot_file();
        let mem_path = paths.memory_file();

        if tokio::fs::metadata(&snapshot_path).await.is_err()
            || tokio::fs::metadata(&mem_path).await.is_err()
        {
            return Ok(SnapshotRestoreOutcome::Missing);
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
            "resume_vm": true
        })
        .to_string();

        if let Err(e) = fc_put_with_timeouts(
            active_socket_path,
            "/snapshot/load",
            &body,
            self.fc_config.api_connect_timeout(),
            self.fc_config.api_status_timeout(),
            self.fc_config.api_header_timeout(),
            self.fc_config.api_body_timeout(),
        )
        .await
        {
            tracing::error!(
                vm_id = %vm_id,
                "Failed to load snapshot: {}. Caller will relaunch Firecracker for a normal boot.",
                e
            );
            Ok(SnapshotRestoreOutcome::Failed)
        } else {
            Ok(SnapshotRestoreOutcome::Restored)
        }
    }

    #[tracing::instrument(skip(self), fields(vm_id = %vm_id))]
    pub(crate) async fn pause_vm(&self, vm_id: &VmId) -> Result<(), HypervisorError> {
        let mut processes = self.processes.lock().await;
        let proc = processes
            .get_mut(vm_id)
            .ok_or_else(|| HypervisorError::VmNotFound(vm_id.to_string()))?;

        tracing::info!(vm_id = %vm_id, "Pausing VM and creating snapshot...");

        let pause_body = serde_json::json!({ "state": "Paused" }).to_string();
        fc_patch_with_timeouts(
            &proc.socket_path,
            "/vm",
            &pause_body,
            self.fc_config.api_connect_timeout(),
            self.fc_config.api_status_timeout(),
            self.fc_config.api_header_timeout(),
            self.fc_config.api_body_timeout(),
        )
        .await?;

        let snapshot_dir = std::path::Path::new(&self.fc_config.data_dir).join("snapshots");
        tokio::fs::create_dir_all(&snapshot_dir)
            .await
            .map_err(|e| {
                HypervisorError::ProcessError(format!("Failed to create snapshots dir: {e}"))
            })?;

        let chroot_dir = proc.chroot_dir.clone();
        let (host_snapshot_path, host_mem_path, snapshot_path, mem_path) =
            self.snapshot_paths(vm_id, chroot_dir.as_deref());
        let snapshot_body = Self::snapshot_create_body(
            &snapshot_path.to_string_lossy(),
            &mem_path.to_string_lossy(),
        );

        fc_put_with_timeouts(
            &proc.socket_path,
            "/snapshot/create",
            &snapshot_body,
            self.fc_config.api_connect_timeout(),
            self.fc_config.api_status_timeout(),
            self.fc_config.api_header_timeout(),
            self.fc_config.api_body_timeout(),
        )
        .await?;

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

        if let Some(ref mut task) = proc.log_task {
            task.abort();
        }

        tracing::info!(vm_id = %vm_id, "Sending kill signal to Firecracker process for hibernation");
        let _ = self.kill_process(vm_id, proc).await;
        tracing::info!(vm_id = %vm_id, "Firecracker process terminated for hibernation");

        let socket_path = proc.socket_path.clone();
        processes.remove(vm_id);
        drop(processes);

        if let Err(e) = tokio::fs::remove_file(&socket_path).await
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::debug!("Failed to remove socket {}: {}", socket_path, e);
        }

        let mut vms = self.vms.write().await;
        if let Some(vm) = vms.get_mut(vm_id) {
            vm.status = VmStatus::Paused;
        }
        drop(vms);
        let _ = self.persist_runtime_state().await;

        tracing::info!(vm_id = %vm_id, "VM paused and process terminated successfully");
        Ok(())
    }

    #[tracing::instrument(skip(self), fields(vm_id = %vm_id))]
    pub(crate) async fn resume_vm(&self, vm_id: &VmId) -> Result<(), HypervisorError> {
        // Inspect process state while holding the processes lock.
        let (socket_path, process_alive, restart_from_snapshot) = {
            let mut processes = self.processes.lock().await;
            let Some(proc) = processes.get_mut(vm_id) else {
                drop(processes);
                tracing::info!(vm_id = %vm_id, "Process missing for resume, attempting restart from snapshot...");
                return self.restart_vm_from_snapshot(vm_id).await;
            };

            let alive = if let Some(child) = proc.child.as_mut() {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        tracing::warn!(
                            vm_id = %vm_id,
                            status = ?status,
                            "Found stale Firecracker process during resume, restarting from snapshot"
                        );
                        false
                    },
                    Ok(None) => true,
                    Err(e) => {
                        tracing::warn!(
                            vm_id = %vm_id,
                            error = %e,
                            "Could not inspect Firecracker process during resume, restarting from snapshot"
                        );
                        false
                    },
                }
            } else if let Some(pid) = proc.pid {
                Self::is_pid_alive(pid)
            } else {
                false
            };
            if !alive {
                (Some(proc.socket_path.clone()), false, true)
            } else {
                (Some(proc.socket_path.clone()), true, false)
            }
        };

        // If the process is still alive, try to resume it in place.
        let mut live_process_resumed = false;
        let mut restart_from_snapshot = restart_from_snapshot;
        if process_alive && let Some(ref socket) = socket_path {
            let resume_body = serde_json::json!({ "state": "Resumed" }).to_string();
            match fc_patch_with_timeouts(
                socket,
                "/vm",
                &resume_body,
                self.fc_config.api_connect_timeout(),
                self.fc_config.api_status_timeout(),
                self.fc_config.api_header_timeout(),
                self.fc_config.api_body_timeout(),
            )
            .await
            {
                Ok(_) => {
                    live_process_resumed = true;
                },
                Err(e) => {
                    tracing::warn!(
                        vm_id = %vm_id,
                        error = %e,
                        "Failed to resume Firecracker process in place, restarting from snapshot"
                    );
                    restart_from_snapshot = true;
                },
            }
        }
        // Update VM status while only holding the VMs lock.
        if live_process_resumed {
            let mut vms = self.vms.write().await;
            if let Some(vm) = vms.get_mut(vm_id) {
                vm.status = VmStatus::Running;
            }
            drop(vms);
            let _ = self.persist_runtime_state().await;
            return Ok(());
        }

        if restart_from_snapshot {
            {
                let mut vms = self.vms.write().await;
                if let Some(vm) = vms.get_mut(vm_id) {
                    vm.status = VmStatus::Stopped;
                }
            }
            {
                let mut processes = self.processes.lock().await;
                processes.remove(vm_id);
            }
            let _ = self.persist_runtime_state().await;
        }

        let vm_info = self
            .get_vm(vm_id)
            .await
            .ok_or_else(|| HypervisorError::VmNotFound(vm_id.to_string()))?;

        let config = vm_info.config.clone();

        self.start_vm(*vm_id, vm_info.app_id, vm_info.image.clone(), config)
            .await?;
        Ok(())
    }

    /// Helper: restart a VM from snapshot (used when process is missing on resume).
    async fn restart_vm_from_snapshot(&self, vm_id: &VmId) -> Result<(), HypervisorError> {
        {
            let mut vms = self.vms.write().await;
            if let Some(vm) = vms.get_mut(vm_id) {
                vm.status = VmStatus::Stopped;
            }
        }
        let _ = self.persist_runtime_state().await;

        let vm_info = self
            .get_vm(vm_id)
            .await
            .ok_or_else(|| HypervisorError::VmNotFound(vm_id.to_string()))?;

        self.start_vm(
            *vm_id,
            vm_info.app_id,
            vm_info.image.clone(),
            vm_info.config.clone(),
        )
        .await
    }
}
