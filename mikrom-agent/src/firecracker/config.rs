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
