use std::path::PathBuf;

/// Centralized path management for Firecracker VM resources.
#[derive(Debug, Clone)]
pub struct VmPaths {
    pub data_dir: PathBuf,
    pub agent_id: String,
    pub vm_id: String,
}

impl VmPaths {
    pub fn new(
        data_dir: impl Into<PathBuf>,
        agent_id: impl Into<String>,
        vm_id: impl Into<String>,
    ) -> Self {
        Self {
            data_dir: data_dir.into(),
            agent_id: agent_id.into(),
            vm_id: vm_id.into(),
        }
    }

    /// Path to the Firecracker API socket.
    pub fn socket_path(&self) -> PathBuf {
        self.data_dir
            .join(format!("fc-{}-{}.sock", self.agent_id, self.vm_id))
    }

    /// Path to the VM's rootfs image on the host.
    pub fn rootfs_path(&self) -> PathBuf {
        self.data_dir
            .join(format!("fc-{}-{}-rootfs.ext4", self.agent_id, self.vm_id))
    }

    /// Path to the VM's metrics JSON file on the host.
    pub fn metrics_path(&self) -> PathBuf {
        self.data_dir
            .join(format!("fc-{}-{}-metrics.json", self.agent_id, self.vm_id))
    }

    /// Path to the VM's chroot directory (if using jailer).
    pub fn chroot_dir(&self) -> PathBuf {
        self.data_dir
            .join(format!("jailer/{}/{}", self.agent_id, self.vm_id))
    }

    /// Path to the snapshots directory.
    pub fn snapshot_dir(&self) -> PathBuf {
        self.data_dir.join("snapshots")
    }

    /// Path to the VM's snapshot file.
    pub fn snapshot_file(&self) -> PathBuf {
        self.snapshot_dir().join(format!("{}.snapshot", self.vm_id))
    }

    /// Path to the VM's memory snapshot file.
    pub fn memory_file(&self) -> PathBuf {
        self.snapshot_dir().join(format!("{}.mem", self.vm_id))
    }
}
