use std::path::PathBuf;

/// Configuration for the QEMU microvm hypervisor.
#[derive(Clone, Debug)]
pub struct QemuConfig {
    /// Path to the QEMU binary (e.g. /usr/bin/qemu-system-x86_64).
    pub binary: String,
    /// Kernel image (vmlinux) for the microvm.
    pub kernel_path: String,
    /// Root filesystem image (ext4 or qcow2).
    pub rootfs_path: String,
    /// Base rootfs used as a template for new VMs.
    pub base_rootfs_path: String,
    /// Data directory for VM state files.
    pub data_dir: PathBuf,
    /// Timeout (seconds) to wait for QMP socket to appear.
    pub qmp_timeout_secs: u64,
    /// Extra arguments appended to every QEMU invocation.
    pub extra_args: Vec<String>,
    /// Optional URL to download kernel image from (overrides kernel_path).
    pub kernel_url: Option<String>,
    /// Optional URL to download rootfs image from (overrides rootfs_path).
    pub rootfs_url: Option<String>,
    /// Directory for caching downloaded images.
    pub image_cache_dir: PathBuf,
    /// Path to virtiofsd binary (empty = disabled).
    pub virtiofsd_binary: String,
    /// Directory for virtiofsd vhost-user sockets.
    pub virtiofsd_socket_dir: PathBuf,
    /// Directories to share via virtiofsd (tag → host path).
    pub virtiofsd_shares: Vec<(String, PathBuf)>,
}

impl QemuConfig {
    pub fn from_env() -> Self {
        Self {
            binary: std::env::var("QEMU_BINARY")
                .unwrap_or_else(|_| "/usr/bin/qemu-system-x86_64".to_string()),
            kernel_path: std::env::var("QEMU_KERNEL_PATH")
                .unwrap_or_else(|_| "/opt/qemu/vmlinux.bin".to_string()),
            rootfs_path: std::env::var("QEMU_ROOTFS_PATH")
                .unwrap_or_else(|_| "/opt/qemu/rootfs.ext4".to_string()),
            base_rootfs_path: std::env::var("QEMU_BASE_ROOTFS")
                .unwrap_or_else(|_| "/opt/qemu/base-rootfs.ext4".to_string()),
            data_dir: std::env::var("QEMU_DATA_DIR")
                .ok()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/var/lib/mikrom/qemu")),
            qmp_timeout_secs: std::env::var("QEMU_QMP_TIMEOUT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10),
            extra_args: Vec::new(),
            kernel_url: std::env::var("QEMU_KERNEL_URL")
                .ok()
                .filter(|s| !s.is_empty()),
            rootfs_url: std::env::var("QEMU_ROOTFS_URL")
                .ok()
                .filter(|s| !s.is_empty()),
            image_cache_dir: std::env::var("QEMU_IMAGE_CACHE_DIR")
                .ok()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/var/cache/mikrom/qemu-images")),
            virtiofsd_binary: std::env::var("VIRTIOFSD_BINARY").unwrap_or_default(),
            virtiofsd_socket_dir: std::env::var("VIRTIOFSD_SOCKET_DIR")
                .ok()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/var/lib/mikrom/virtiofsd")),
            virtiofsd_shares: Vec::new(),
        }
    }
}
