use mikrom_proto::id::VmId;
use std::path::PathBuf;

/// Centralized path management for Firecracker VM resources.
pub struct VmPaths {
    pub data_dir: PathBuf,
    pub agent_id: String,
    pub vm_id: VmId,
}

impl VmPaths {
    pub fn new(data_dir: impl Into<PathBuf>, agent_id: impl Into<String>, vm_id: VmId) -> Self {
        Self {
            data_dir: data_dir.into(),
            agent_id: agent_id.into(),
            vm_id,
        }
    }

    /// Path to the Firecracker API socket.
    pub fn socket_path(&self) -> PathBuf {
        if self.data_dir.as_os_str().is_empty() {
            PathBuf::from(format!("/tmp/fc-{}.socket", self.vm_id))
        } else {
            self.data_dir.join(format!("fc-{}.socket", self.vm_id))
        }
    }

    /// Path to the rootfs disk image.
    pub fn rootfs_path(&self) -> PathBuf {
        self.data_dir
            .join(format!("fc-{}-{}-rootfs.ext4", self.agent_id, self.vm_id))
    }

    /// Path to the VM's configuration JSON file.
    pub fn config_path(&self) -> PathBuf {
        self.data_dir.join(format!("fc-{}.json", self.vm_id))
    }

    /// Path to the Firecracker process logs.
    pub fn log_path(&self) -> PathBuf {
        self.data_dir.join(format!("fc-{}.log", self.vm_id))
    }

    /// Path to the Firecracker stdout log.
    pub fn stdout_log_path(&self) -> PathBuf {
        self.data_dir.join(format!("fc-{}.stdout.log", self.vm_id))
    }

    /// Path to the Firecracker stderr log.
    pub fn stderr_log_path(&self) -> PathBuf {
        self.data_dir.join(format!("fc-{}.stderr.log", self.vm_id))
    }

    /// Path to the Firecracker metrics pipe/file.
    pub fn metrics_path(&self) -> PathBuf {
        self.data_dir.join(format!("fc-{}.metrics", self.vm_id))
    }

    pub fn snapshot_file(&self) -> PathBuf {
        self.data_dir
            .join("snapshots")
            .join(format!("{}.snapshot", self.vm_id))
    }

    pub fn memory_file(&self) -> PathBuf {
        self.data_dir
            .join("snapshots")
            .join(format!("{}.mem", self.vm_id))
    }

    pub fn vfs_socket_path(&self, vol_id: &str) -> PathBuf {
        let safe_id: String = vol_id
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.data_dir
            .join(format!("vfs-{}-{}.socket", self.vm_id, safe_id))
    }
}
