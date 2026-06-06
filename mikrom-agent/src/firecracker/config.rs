#[derive(Clone, Debug)]
pub struct FirecrackerConfig {
    pub kernel_path: Option<String>,
    pub binary: String,
    pub rootfs_path: String,
    pub base_rootfs_path: String,
    pub data_dir: String,
    pub use_jailer: bool,
    pub jailer_binary: String,
    pub jailer_uid: u32,
    pub jailer_gid: u32,
    pub chroot_base: String,
    pub virtiofsd_path: String,
}

impl FirecrackerConfig {
    fn timeout_duration_env(name: &str, default_secs: u64) -> std::time::Duration {
        let secs = std::env::var(name)
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(default_secs);
        std::time::Duration::from_secs(secs.max(1))
    }

    pub fn socket_wait_timeout_plain(&self) -> std::time::Duration {
        Self::timeout_duration_env("FC_SOCKET_WAIT_TIMEOUT_SECS", 120)
    }

    pub fn socket_wait_timeout_chroot(&self) -> std::time::Duration {
        Self::timeout_duration_env("FC_SOCKET_WAIT_TIMEOUT_CHROOT_SECS", 10)
    }

    pub fn api_connect_timeout(&self) -> std::time::Duration {
        Self::timeout_duration_env("FC_API_CONNECT_TIMEOUT_SECS", 2)
    }

    pub fn api_status_timeout(&self) -> std::time::Duration {
        Self::timeout_duration_env("FC_API_STATUS_TIMEOUT_SECS", 30)
    }

    pub fn api_header_timeout(&self) -> std::time::Duration {
        Self::timeout_duration_env("FC_API_HEADER_TIMEOUT_SECS", 10)
    }

    pub fn api_body_timeout(&self) -> std::time::Duration {
        Self::timeout_duration_env("FC_API_BODY_TIMEOUT_SECS", 60)
    }

    pub fn process_terminate_timeout(&self) -> std::time::Duration {
        Self::timeout_duration_env("FC_PROCESS_TERMINATE_TIMEOUT_SECS", 10)
    }

    pub fn process_kill_timeout(&self) -> std::time::Duration {
        Self::timeout_duration_env("FC_PROCESS_KILL_TIMEOUT_SECS", 2)
    }

    pub fn vfs_terminate_timeout(&self) -> std::time::Duration {
        Self::timeout_duration_env("FC_VFS_TERMINATE_TIMEOUT_SECS", 5)
    }

    pub fn vfs_kill_timeout(&self) -> std::time::Duration {
        Self::timeout_duration_env("FC_VFS_KILL_TIMEOUT_SECS", 2)
    }

    #[must_use]
    pub fn from_env() -> Self {
        Self {
            kernel_path: Some(
                std::env::var("FC_KERNEL_PATH")
                    .unwrap_or_else(|_| "/opt/firecracker/vmlinux.bin".to_string()),
            ),
            binary: std::env::var("FC_BINARY")
                .unwrap_or_else(|_| "/usr/bin/firecracker".to_string()),
            rootfs_path: std::env::var("FC_ROOTFS_PATH")
                .unwrap_or_else(|_| "/opt/firecracker/rootfs.ext4".to_string()),
            base_rootfs_path: std::env::var("FC_BASE_ROOTFS")
                .unwrap_or_else(|_| "/opt/firecracker/base-rootfs.ext4".to_string()),
            data_dir: std::env::var("FC_DATA_DIR")
                .unwrap_or_else(|_| "/var/lib/mikrom/data".to_string()),
            use_jailer: std::env::var("USE_JAILER").is_ok_and(|v| v == "true"),
            jailer_binary: std::env::var("JAILER_BINARY")
                .unwrap_or_else(|_| "/usr/bin/jailer".to_string()),
            jailer_uid: std::env::var("JAILER_UID")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1000),
            jailer_gid: std::env::var("JAILER_GID")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1000),
            chroot_base: std::env::var("JAILER_CHROOT_BASE")
                .unwrap_or_else(|_| "/srv/jailer".to_string()),
            virtiofsd_path: std::env::var("VIRTIOFSD_PATH")
                .unwrap_or_else(|_| "/usr/libexec/virtiofsd".to_string()),
        }
    }

    #[must_use]
    pub fn stub() -> Self {
        Self {
            kernel_path: None,
            binary: String::new(),
            rootfs_path: String::new(),
            base_rootfs_path: String::new(),
            data_dir: "/tmp/mikrom-stub-data".to_string(),
            use_jailer: false,
            jailer_binary: String::new(),
            jailer_uid: 0,
            jailer_gid: 0,
            chroot_base: String::new(),
            virtiofsd_path: String::new(),
        }
    }
}
