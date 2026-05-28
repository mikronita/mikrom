use crate::hypervisor::{HypervisorError, VmStatus};
use crate::qemu::manager::QemuManager;
use mikrom_proto::id::VmId;
use std::os::unix::process::ExitStatusExt;
use std::time::Duration;

impl QemuManager {
    fn is_pid_alive(&self, pid: u32) -> bool {
        let mut system = sysinfo::System::new();
        system.refresh_processes(
            sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from(pid as usize)]),
            true,
        );
        if let Some(process) = system.process(sysinfo::Pid::from(pid as usize)) {
            process.name().to_string_lossy().contains("qemu-system")
        } else {
            false
        }
    }

    async fn remove_file_best_effort(path: &std::path::Path, context: &'static str) {
        if let Err(e) = tokio::fs::remove_file(path).await {
            tracing::debug!(path = %path.display(), error = %e, %context, "Best-effort file removal failed");
        }
    }

    async fn kill_child_best_effort(
        child: &mut tokio::process::Child,
        context: &'static str,
        vm_id: &VmId,
    ) {
        if let Err(e) = child.kill().await {
            tracing::debug!(vm_id = %vm_id, error = %e, %context, "Best-effort child kill failed");
        }
        if let Err(e) = child.wait().await {
            tracing::debug!(vm_id = %vm_id, error = %e, %context, "Best-effort child wait failed");
        }
    }

    pub(crate) async fn run_gc(&self) {
        let mut processes = self.processes.lock().await;
        let mut to_restart = Vec::new();
        let mut to_remove = Vec::new();

        for (vm_id, proc) in processes.iter_mut() {
            let exited = match proc.child.try_wait() {
                Ok(Some(status)) => {
                    tracing::info!(
                        vm_id = %vm_id,
                        code = ?status.code(),
                        signal = ?status.signal(),
                        "QEMU process exited"
                    );
                    Some(status)
                },
                Ok(None) => None,
                Err(e) => {
                    tracing::error!(vm_id = %vm_id, error = %e, "Error checking QEMU process");
                    None
                },
            };

            if exited.is_some() {
                to_remove.push(*vm_id);
                let vms = self.vms.read().await;
                if let Some(vm) = vms.get(vm_id)
                    && (vm.status == VmStatus::Running || vm.status == VmStatus::Starting)
                {
                    tracing::error!(
                        vm_id = %vm_id,
                        "QEMU process exited unexpectedly, scheduling restart"
                    );
                    to_restart.push((*vm_id, vm.app_id, vm.image.clone(), vm.config.clone()));
                }
            }
        }

        for vm_id in &to_remove {
            processes.remove(vm_id);
        }
        drop(processes);

        for (vm_id, app_id, image, config) in to_restart {
            tracing::info!(vm_id = %vm_id, "Auto-restarting QEMU VM after unexpected exit");
            if let Err(e) = self.start_vm(vm_id, app_id, image, config).await {
                tracing::error!(vm_id = %vm_id, error = %e, "Auto-restart failed");
            }
        }

        self.cleanup_all_stale_resources().await;
    }

    pub async fn stop_vm(&self, vm_id: &VmId) -> Result<(), HypervisorError> {
        // Phase 1: Extract the process and release the lock before any
        // potentially-blocking I/O or before acquiring the vms lock.
        let proc_data = {
            let mut procs = self.processes.lock().await;
            procs.remove(vm_id)
        };

        if let Some(mut proc) = proc_data {
            if let Some(task) = proc.event_task.take() {
                task.abort();
            }

            let qmp_path = proc.qmp_socket.clone();

            // Try graceful shutdown via QMP first
            let qmp_ok = if let Some(ref qmp) = proc.qmp {
                let mut qmp = qmp.lock().await;
                qmp.system_powerdown().await.is_ok() || qmp.quit().await.is_ok()
            } else {
                false
            };

            if !qmp_ok {
                Self::kill_child_best_effort(&mut proc.child, "qemu-stop-child", vm_id).await;
            } else {
                tokio::time::sleep(Duration::from_millis(500)).await;
                Self::kill_child_best_effort(&mut proc.child, "qemu-stop-child-after-qmp", vm_id)
                    .await;
            }

            for mut fsd in proc.virtiofsd {
                Self::kill_child_best_effort(&mut fsd, "qemu-stop-virtiofsd", vm_id).await;
            }
            for sock in &proc.virtiofsd_sockets {
                Self::remove_file_best_effort(sock, "qemu-stop-virtiofsd-socket").await;
            }

            Self::remove_file_best_effort(&qmp_path, "qemu-stop-qmp").await;
            Self::remove_file_best_effort(&self.pidfile_path(vm_id), "qemu-stop-pidfile").await;
        }

        let mut vms = self.vms.write().await;
        if let Some(vm) = vms.get_mut(vm_id) {
            vm.status = VmStatus::Stopped;
        } else {
            return Err(HypervisorError::VmNotFound(vm_id.to_string()));
        }
        Ok(())
    }

    pub async fn delete_vm(&self, vm_id: &VmId) -> Result<(), HypervisorError> {
        // 1. Best-effort stop.
        let _ = self.stop_vm(vm_id).await;

        // Cleanup any orphaned processes based on PID file
        let pidfile = self.pidfile_path(vm_id);
        if pidfile.exists()
            && let Ok(pid_str) = std::fs::read_to_string(&pidfile)
            && let Ok(pid) = pid_str.trim().parse::<u32>()
            && self.is_pid_alive(pid)
        {
            tracing::info!(vm_id = %vm_id, pid = pid, "Killing orphaned QEMU process");
            let _ = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            tokio::time::sleep(Duration::from_millis(500)).await;
            let _ = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        }

        if pidfile.exists() {
            let _ = tokio::fs::remove_file(&pidfile).await;
        }

        // 2. Finally remove from memory and disk
        let mut vms = self.vms.write().await;
        vms.remove(vm_id);
        self.logs.remove(vm_id);
        Self::remove_file_best_effort(&self.vm_state_path(vm_id), "qemu-delete-vm-state").await;

        Ok(())
    }

    pub async fn restart_vm(&self, vm_id: &VmId) -> Result<(), HypervisorError> {
        let vm_info = {
            let vms = self.vms.read().await;
            vms.get(vm_id).cloned()
        };
        let Some(info) = vm_info else {
            return Err(HypervisorError::VmNotFound(vm_id.to_string()));
        };
        self.stop_vm(vm_id).await.ok();
        tokio::time::sleep(Duration::from_millis(500)).await;
        self.start_vm(*vm_id, info.app_id, info.image, info.config)
            .await
    }

    pub async fn cleanup_all_stale_resources(&self) {
        let active_vms: Vec<VmId> = {
            let vms = self.vms.read().await;
            vms.keys().cloned().collect()
        };

        let active_set: std::collections::HashSet<String> =
            active_vms.iter().map(|id| id.to_string()).collect();

        // Clean up data_dir (state JSON, pidfiles, QMP sockets, serial logs)
        let data_dir = &self.config.data_dir;
        if data_dir.exists()
            && let Ok(mut entries) = tokio::fs::read_dir(data_dir).await
        {
            let mut paths = Vec::new();
            while let Ok(Some(entry)) = entries.next_entry().await {
                paths.push(entry.path());
            }
            for path in paths {
                let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
                    continue;
                };
                let Some(rest) = file_name.strip_prefix("qemu-") else {
                    continue;
                };
                let vm_id_str = rest
                    .strip_suffix(".err.log")
                    .or_else(|| rest.strip_suffix(".log"))
                    .or_else(|| rest.strip_suffix(".pid"))
                    .or_else(|| rest.strip_suffix(".qmp"))
                    .or_else(|| rest.strip_suffix(".json"))
                    .unwrap_or(rest)
                    .to_string();

                if active_set.contains(&vm_id_str) {
                    continue;
                }

                // Stale resource – remove it
                if path.extension().is_some_and(|ext| ext == "pid")
                    && let Ok(pid_str) = std::fs::read_to_string(&path)
                    && let Ok(pid) = pid_str.trim().parse::<u32>()
                {
                    #[cfg(unix)]
                    // SAFETY: valid PID from our own pidfile
                    unsafe {
                        libc::kill(pid as i32, libc::SIGKILL);
                    }
                }

                if let Err(e) = tokio::fs::remove_file(&path).await {
                    tracing::debug!(
                        path = %path.display(),
                        error = %e,
                        "Failed to remove stale resource"
                    );
                } else {
                    tracing::info!(
                        path = %path.display(),
                        vm_id = %vm_id_str,
                        "Cleaned up stale resource"
                    );
                }
            }
        }

        // Clean up virtiofsd socket directory
        let fsd_dir = &self.config.virtiofsd_socket_dir;
        if fsd_dir.exists()
            && let Ok(mut entries) = tokio::fs::read_dir(fsd_dir).await
        {
            let mut paths = Vec::new();
            while let Ok(Some(entry)) = entries.next_entry().await {
                paths.push(entry.path());
            }
            for path in paths {
                let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                // Expected format: "qemu-{vm_id}-{tag}"
                let vm_id_str = if name.starts_with("qemu-") {
                    let remainder = name.strip_prefix("qemu-").unwrap_or(name);
                    // Split on first '-' to get vm_id part
                    if let Some((vm_id, _)) = remainder.split_once('-') {
                        vm_id.to_string()
                    } else {
                        continue;
                    }
                } else {
                    continue;
                };

                if active_set.contains(&vm_id_str) {
                    continue;
                }

                if let Err(e) = tokio::fs::remove_file(&path).await {
                    tracing::debug!(
                        path = %path.display(),
                        error = %e,
                        "Failed to remove stale virtiofsd socket"
                    );
                } else {
                    tracing::info!(
                        path = %path.display(),
                        vm_id = %vm_id_str,
                        "Cleaned up stale virtiofsd socket"
                    );
                }
            }
        }
    }
}
