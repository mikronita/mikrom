use crate::hypervisor::{HypervisorError, VmConfig, VmInfo, VmStatus};
use crate::qemu::manager::QemuManager;
use crate::qemu::qmp::QmpClient;
use mikrom_proto::id::{AppId, VmId};
use std::path::PathBuf;
use std::time::Duration;

impl QemuManager {
    pub async fn start_vm(
        &self,
        vm_id: VmId,
        app_id: AppId,
        image: String,
        config: VmConfig,
    ) -> Result<(), HypervisorError> {
        let kernel = self.resolve_kernel().await;
        let kernel_str = kernel.to_string_lossy().to_string();
        let rootfs = self.resolve_rootfs(&image).await;
        let rootfs_str = rootfs.to_string_lossy().to_string();
        let tap_name = Self::tap_name(&vm_id);
        let qmp_socket = self.qmp_socket_path(&vm_id);
        let qmp_str = qmp_socket.to_string_lossy().to_string();
        let pidfile = self.pidfile_path(&vm_id);
        let pidfile_str = pidfile.to_string_lossy().to_string();
        let serial_log = self.serial_log_path(&vm_id);
        let serial_log_str = serial_log.to_string_lossy().to_string();

        // Ensure directories exist
        if let Some(parent) = qmp_socket.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        // Delete stale pidfile if present
        let _ = tokio::fs::remove_file(&pidfile).await;
        let _ = tokio::fs::remove_file(&qmp_socket).await;

        let args = self.build_qemu_cmd(
            &vm_id,
            &config,
            &kernel_str,
            &rootfs_str,
            &tap_name,
            &qmp_str,
            &pidfile_str,
            &serial_log_str,
        );

        // The binary is the first argument; rest are args
        let binary = &args[0];
        let qemu_args = &args[1..];

        // Redirect stderr to a dedicated log file for structured debugging
        let stderr_log = self.stderr_log_path(&vm_id);
        let stderr_file = std::fs::File::create(&stderr_log).map_err(|e| {
            HypervisorError::ProcessError(format!(
                "Failed to create stderr log {}: {e}",
                stderr_log.display()
            ))
        })?;

        let child = tokio::process::Command::new(binary)
            .args(qemu_args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::from(stderr_file))
            .spawn()
            .map_err(|e| HypervisorError::ProcessError(format!("Failed to spawn QEMU: {e}")))?;

        let pid = child
            .id()
            .ok_or_else(|| HypervisorError::ProcessError("Failed to get QEMU PID".to_string()))?;

        // Wait a bit for the pidfile to be written by QEMU
        tokio::time::sleep(Duration::from_millis(500)).await;
        let qemu_pid = Self::read_pidfile(&pidfile).await.unwrap_or(pid);

        // Spawn virtiofsd processes for each share
        let mut virtiofsd_children = Vec::new();
        let mut virtiofsd_socks = Vec::new();
        if !self.config.virtiofsd_binary.is_empty() {
            if let Some(parent) = self.config.virtiofsd_socket_dir.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            for (tag, host_path) in &self.config.virtiofsd_shares {
                let sock = self.virtiofsd_socket_path(&vm_id, tag);
                let _ = tokio::fs::remove_file(&sock).await;
                let sock_str = sock.to_string_lossy().to_string();
                let host_str = host_path.to_string_lossy().to_string();
                match tokio::process::Command::new(&self.config.virtiofsd_binary)
                    .args([
                        "--socket-path",
                        &sock_str,
                        "--shared-dir",
                        &host_str,
                        "--cache",
                        "auto",
                        "--sandbox",
                        "none",
                    ])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                {
                    Ok(child) => {
                        virtiofsd_children.push(child);
                        virtiofsd_socks.push(sock);
                        tracing::info!(tag = %tag, socket = %sock_str, "virtiofsd started");
                    },
                    Err(e) => {
                        tracing::warn!(
                            tag = %tag,
                            error = %e,
                            "Failed to start virtiofsd (continuing without)"
                        );
                    },
                }
            }
        }

        // Connect to QMP (best-effort based on timeout)
        let qmp = if Self::wait_for_qmp(&qmp_socket, self.config.qmp_timeout_secs)
            .await
            .is_ok()
        {
            match QmpClient::connect(&qmp_socket).await {
                Ok(client) => Some(tokio::sync::Mutex::new(client)),
                Err(e) => {
                    tracing::warn!(
                        vm_id = %vm_id,
                        error = %e,
                        "Failed to connect QMP (continuing without QMP)"
                    );
                    None
                },
            }
        } else {
            tracing::warn!(
                vm_id = %vm_id,
                "QEMU QMP socket not ready (continuing without QMP)"
            );
            None
        };

        // Spawn QMP event listener if the socket is available
        let event_task = if qmp.is_some() {
            Some(self.spawn_event_listener(vm_id, qmp_socket.clone()))
        } else {
            None
        };

        let now = chrono::Utc::now().timestamp();
        let vm_info = VmInfo {
            vm_id,
            app_id,
            image,
            config: config.clone(),
            status: VmStatus::Running,
            started_at: Some(now),
            error_message: None,
        };
        let vm_id_key = vm_info.vm_id;

        let qemu_proc = crate::qemu::manager::QemuProcess {
            child,
            pid: qemu_pid,
            qmp_socket: qmp_socket.clone(),
            tap_name: tap_name.clone(),
            started_at: now,
            qmp,
            virtiofsd: virtiofsd_children,
            virtiofsd_sockets: virtiofsd_socks,
            event_task,
        };

        self.vms.write().await.insert(vm_id_key, vm_info);
        self.processes.lock().await.insert(vm_id_key, qemu_proc);

        tracing::info!(
            vm_id = %vm_id_key,
            pid = qemu_pid,
            tap = %tap_name,
            "QEMU microvm started"
        );

        Ok(())
    }

    pub async fn init_network(&self) -> Result<(), HypervisorError> {
        crate::network::ensure_bridge().await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_qemu_cmd(
        &self,
        vm_id: &VmId,
        config: &VmConfig,
        kernel: &str,
        rootfs: &str,
        tap_name: &str,
        qmp_socket: &str,
        pidfile: &str,
        serial_log: &str,
    ) -> Vec<String> {
        let mem_mb = if config.memory_mib > 0 {
            config.memory_mib.to_string()
        } else {
            "128".to_string()
        };
        let vcpus = if config.vcpus > 0 {
            config.vcpus.to_string()
        } else {
            "1".to_string()
        };

        let mut args = vec![
            self.config.binary.clone(),
            "-machine".into(),
            "microvm".into(),
            "-cpu".into(),
            "host".into(),
            "-smp".into(),
            vcpus,
            "-m".into(),
            mem_mb,
            "-no-reboot".into(),
            "-nographic".into(),
            "-serial".into(),
            format!("file:{}", serial_log),
            "-kernel".into(),
            kernel.to_string(),
            "-drive".into(),
            format!("file={rootfs},format=raw,if=virtio,id=root"),
            "-device".into(),
            "virtio-blk-device,drive=root".into(),
            "-netdev".into(),
            format!("tap,id=net0,ifname={tap_name},script=no,downscript=no"),
            "-device".into(),
            "virtio-net-device,netdev=net0".into(),
            "-device".into(),
            format!("vhost-vsock-device,guest-cid={}", Self::vsock_cid(vm_id)),
            "-qmp".into(),
            format!("unix:{qmp_socket},server,nowait"),
            "-pidfile".into(),
            pidfile.into(),
            "-append".into(),
            "console=ttyS0 root=/dev/vda reboot=t panic=1".into(),
        ];

        // Add virtiofs devices
        for (tag, _host_path) in &self.config.virtiofsd_shares {
            let sock = self.virtiofsd_socket_path(vm_id, tag);
            args.push("-chardev".into());
            args.push(format!("socket,id=char-{tag},path={}", sock.display()));
            args.push("-device".into());
            args.push(format!("vhost-user-fs-device,chardev=char-{tag},tag={tag}"));
        }

        // Add extra args from config
        args.extend(self.config.extra_args.clone());

        args
    }

    pub(crate) async fn wait_for_qmp(
        path: &PathBuf,
        timeout_secs: u64,
    ) -> Result<(), HypervisorError> {
        let start = tokio::time::Instant::now();
        loop {
            if tokio::fs::metadata(path).await.is_ok() {
                return Ok(());
            }
            if start.elapsed() > Duration::from_secs(timeout_secs) {
                return Err(HypervisorError::ProcessError(
                    "QMP socket did not appear within timeout".to_string(),
                ));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}
