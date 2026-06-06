use crate::firecracker::guard::VmStartupGuard;
use crate::firecracker::paths::VmPaths;
use crate::hypervisor::HypervisorError;
use mikrom_proto::id::VmId;
use std::ffi::CString;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64},
};
use tokio::io::AsyncReadExt;

impl crate::firecracker::FirecrackerManager {
    pub(crate) async fn resolve_startup_context(
        &self,
        vm_id: &VmId,
        kernel_path: &str,
        rootfs_path: &std::path::Path,
        paths: &VmPaths,
    ) -> Result<crate::firecracker::manager::StartupContext, HypervisorError> {
        if self.fc_config.use_jailer {
            let (bin, args, host_socket, chroot) = self
                .setup_jailer(vm_id, kernel_path, &rootfs_path.to_string_lossy())
                .await?;

            self.remove_stale_socket(&host_socket).await;
            Ok(crate::firecracker::manager::StartupContext {
                exec_binary: bin,
                exec_args: args,
                active_socket_path: host_socket,
                chroot_dir: chroot,
            })
        } else {
            let socket_path = paths.socket_path();
            self.remove_stale_socket(&socket_path).await;
            Ok(crate::firecracker::manager::StartupContext {
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

    pub(crate) async fn setup_jailer(
        &self,
        vm_id: &VmId,
        kernel_host_path: &str,
        rootfs_host_path: &str,
    ) -> Result<(String, Vec<String>, String, Option<String>), HypervisorError> {
        let chroot_dir = self.get_chroot_dir(vm_id);

        if chroot_dir.exists() {
            tracing::info!(vm_id = %vm_id, ?chroot_dir, "Cleaning up existing jailer chroot before setup");
            let _ = tokio::fs::remove_dir_all(&chroot_dir).await;
        }

        let root_dir = chroot_dir.join("root");
        let run_dir = root_dir.join("run");

        tokio::fs::create_dir_all(&run_dir).await.map_err(|e| {
            HypervisorError::ProcessError(format!(
                "Failed to create jailer directory {:?}: {}",
                run_dir, e
            ))
        })?;

        let kernel_filename = "vmlinux.bin";
        let rootfs_filename = "rootfs.ext4";

        let chroot_kernel_path = root_dir.join(kernel_filename);
        let chroot_rootfs_path = root_dir.join(rootfs_filename);

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

    pub(crate) async fn spawn_firecracker_process(
        &self,
        startup: &crate::firecracker::manager::StartupContext,
        paths: &VmPaths,
    ) -> Result<tokio::process::Child, HypervisorError> {
        let stdout_path = paths.stdout_log_path();
        let stderr_path = paths.stderr_log_path();
        if let Some(parent) = stdout_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                HypervisorError::ProcessError(format!("Failed to create log directory: {e}"))
            })?;
        }

        let stdout_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&stdout_path)
            .map_err(|e| {
                HypervisorError::ProcessError(format!(
                    "Failed to open firecracker stdout log {}: {e}",
                    stdout_path.display()
                ))
            })?;
        let stderr_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&stderr_path)
            .map_err(|e| {
                HypervisorError::ProcessError(format!(
                    "Failed to open firecracker stderr log {}: {e}",
                    stderr_path.display()
                ))
            })?;

        tokio::process::Command::new(&startup.exec_binary)
            .args(&startup.exec_args)
            .stdout(std::process::Stdio::from(stdout_file))
            .stderr(std::process::Stdio::from(stderr_file))
            .spawn()
            .map_err(|e| {
                let msg = format!(
                    "Failed to spawn firecracker process (binary: {}): {e}",
                    startup.exec_binary
                );
                tracing::error!("{}", msg);
                HypervisorError::ProcessError(msg)
            })
    }

    pub(crate) async fn wait_for_firecracker_socket(
        &self,
        startup: &crate::firecracker::manager::StartupContext,
    ) -> Result<(), HypervisorError> {
        let wait_timeout = if startup.chroot_dir.is_some() {
            self.fc_config.socket_wait_timeout_chroot()
        } else {
            self.fc_config.socket_wait_timeout_plain()
        };

        crate::firecracker::api::wait_for_socket(&startup.active_socket_path, wait_timeout).await?;
        Ok(())
    }

    pub(crate) fn build_startup_guard(
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

    pub(crate) fn get_chroot_dir(&self, vm_id: &VmId) -> PathBuf {
        let exec_name = std::path::Path::new(&self.fc_config.binary)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("firecracker");

        std::path::Path::new(&self.fc_config.chroot_base)
            .join(exec_name)
            .join(vm_id.to_string())
    }

    pub(crate) async fn validate_kernel_image(
        &self,
        kernel_path: &str,
    ) -> Result<(), HypervisorError> {
        let mut kernel = tokio::fs::File::open(kernel_path).await.map_err(|e| {
            HypervisorError::ProcessError(format!(
                "Failed to open kernel image at {kernel_path}: {e}"
            ))
        })?;
        let mut magic = [0u8; 4];
        kernel.read_exact(&mut magic).await.map_err(|e| {
            HypervisorError::ProcessError(format!(
                "Failed to read kernel header at {kernel_path}: {e}"
            ))
        })?;

        if magic != [0x7f, b'E', b'L', b'F'] {
            return Err(HypervisorError::ProcessError(format!(
                "Invalid kernel image at {kernel_path}: expected an uncompressed ELF Linux kernel, but the file does not start with ELF magic"
            )));
        }

        Ok(())
    }

    pub(crate) async fn mknod_at(&self, dev_path: &str, dst: &str) -> Result<(), HypervisorError> {
        tracing::info!("Creating block device node: {} -> {}", dev_path, dst);

        let dev_path = dev_path.to_string();
        let dst = dst.to_string();
        tokio::task::spawn_blocking(move || -> Result<(), HypervisorError> {
            let metadata = fs::metadata(&dev_path).map_err(|e| {
                HypervisorError::ProcessError(format!("Failed to stat device {dev_path}: {e}"))
            })?;
            let dev = metadata.rdev();
            let major = libc::major(dev);
            let minor = libc::minor(dev);

            let path = CString::new(dst.as_str()).map_err(|e| {
                HypervisorError::ProcessError(format!("Invalid device node path {dst}: {e}"))
            })?;
            let mode = libc::S_IFBLK | 0o600;
            let device = libc::makedev(major, minor);

            let rc = unsafe { libc::mknod(path.as_ptr(), mode, device) };
            if rc != 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::AlreadyExists {
                    return Ok(());
                }
                return Err(HypervisorError::ProcessError(format!(
                    "mknod failed: {err}"
                )));
            }

            Ok(())
        })
        .await
        .map_err(|e| HypervisorError::ProcessError(format!("Failed to create device node: {e}")))?
    }

    pub(crate) async fn ensure_file_at(&self, src: &str, dst: &str) -> Result<(), HypervisorError> {
        let canonical_src = tokio::fs::canonicalize(src).await.map_err(|e| {
            HypervisorError::ProcessError(format!("Failed to resolve path {src}: {e}"))
        })?;

        if let Err(_e) = tokio::fs::hard_link(&canonical_src, dst).await {
            tokio::fs::copy(&canonical_src, dst).await.map_err(|e| {
                HypervisorError::ProcessError(format!(
                    "Failed to copy file from {canonical_src:?} to {dst}: {e}"
                ))
            })?;
        }
        Ok(())
    }

    pub(crate) async fn copy_file_at(&self, src: &str, dst: &str) -> Result<(), HypervisorError> {
        let canonical_src = tokio::fs::canonicalize(src).await.map_err(|e| {
            HypervisorError::ProcessError(format!("Failed to resolve path {src}: {e}"))
        })?;

        tokio::fs::copy(&canonical_src, dst).await.map_err(|e| {
            HypervisorError::ProcessError(format!(
                "Failed to copy file from {canonical_src:?} to {dst}: {e}"
            ))
        })?;
        Ok(())
    }

    pub(crate) async fn recursive_chown(
        &self,
        path: &str,
        uid: u32,
        gid: u32,
    ) -> Result<(), HypervisorError> {
        let path = PathBuf::from(path);
        tokio::task::spawn_blocking(move || -> Result<(), HypervisorError> {
            use std::os::unix::fs as unix_fs;
            let mut stack = vec![path];

            while let Some(current_path) = stack.pop() {
                unix_fs::lchown(&current_path, Some(uid), Some(gid)).map_err(|e| {
                    HypervisorError::ProcessError(format!("Failed to chown {current_path:?}: {e}"))
                })?;

                let metadata = std::fs::symlink_metadata(&current_path).map_err(|e| {
                    HypervisorError::ProcessError(format!(
                        "Failed to get metadata for {current_path:?}: {e}"
                    ))
                })?;

                if metadata.is_dir() {
                    let entries = std::fs::read_dir(&current_path).map_err(|e| {
                        HypervisorError::ProcessError(format!(
                            "Failed to read directory {current_path:?}: {e}"
                        ))
                    })?;

                    for entry in entries {
                        let entry = entry.map_err(|e| {
                            HypervisorError::ProcessError(format!(
                                "Failed to get next entry in {current_path:?}: {e}"
                            ))
                        })?;
                        stack.push(entry.path());
                    }
                }
            }
            Ok(())
        })
        .await
        .map_err(|e| HypervisorError::ProcessError(format!("Blocking task failed: {e}")))?
    }
}
