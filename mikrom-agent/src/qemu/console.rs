use crate::qemu::manager::QemuManager;
use mikrom_proto::id::VmId;
use std::path::PathBuf;

impl QemuManager {
    pub(crate) fn serial_log_path(&self, vm_id: &VmId) -> PathBuf {
        self.config.data_dir.join(format!("qemu-{vm_id}.log"))
    }

    pub(crate) fn stderr_log_path(&self, vm_id: &VmId) -> PathBuf {
        self.config.data_dir.join(format!("qemu-{vm_id}.err.log"))
    }

    /// Read the serial console log for a VM.
    ///
    /// Returns the last `max_lines` lines from the serial log file,
    /// or an empty vector if the VM has no serial log.
    pub async fn get_serial_console(&self, vm_id: &VmId, max_lines: usize) -> Vec<String> {
        let path = self.serial_log_path(vm_id);
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => {
                let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
                let n = lines.len();
                if n <= max_lines {
                    lines
                } else {
                    lines.into_iter().skip(n - max_lines).collect()
                }
            },
            Err(e) => {
                tracing::debug!(
                    vm_id = %vm_id,
                    path = %path.display(),
                    error = %e,
                    "No serial console log available"
                );
                Vec::new()
            },
        }
    }

    /// Read QEMU stderr logs for a VM.
    ///
    /// Returns the last `max_lines` lines from the stderr log file,
    /// or an empty vector if the VM has no stderr log.
    pub async fn get_stderr_logs(&self, vm_id: &VmId, max_lines: usize) -> Vec<String> {
        let path = self.stderr_log_path(vm_id);
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => {
                let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
                let n = lines.len();
                if n <= max_lines {
                    lines
                } else {
                    lines.into_iter().skip(n - max_lines).collect()
                }
            },
            Err(e) => {
                tracing::debug!(
                    vm_id = %vm_id,
                    path = %path.display(),
                    error = %e,
                    "No stderr log available"
                );
                Vec::new()
            },
        }
    }
}
