use crate::hypervisor::HypervisorError;
use crate::qemu::manager::QemuManager;
use mikrom_proto::id::VmId;

impl QemuManager {
    /// Create a VM snapshot using QEMU's `savevm` HMP command.
    ///
    /// The snapshot captures both RAM and disk state.  Requires a QMP connection.
    pub async fn create_snapshot(&self, vm_id: &VmId, name: &str) -> Result<(), HypervisorError> {
        with_qmp!(self, vm_id, "snapshot", |qmp| {
            let cmd = format!("savevm {name}");
            qmp.human_monitor_command(&cmd).await.map_err(|e| {
                HypervisorError::ProcessError(format!("Failed to create snapshot: {e}"))
            })?;
            tracing::info!(vm_id = %vm_id, snapshot = %name, "Snapshot created");
            Ok(())
        })
    }

    /// Restore a VM snapshot using QEMU's `loadvm` HMP command.
    ///
    /// Reverts both RAM and disk to the saved state.  The VM must be stopped first.
    pub async fn restore_snapshot(&self, vm_id: &VmId, name: &str) -> Result<(), HypervisorError> {
        with_qmp!(self, vm_id, "snapshot restore", |qmp| {
            let cmd = format!("loadvm {name}");
            qmp.human_monitor_command(&cmd).await.map_err(|e| {
                HypervisorError::ProcessError(format!("Failed to restore snapshot: {e}"))
            })?;
            tracing::info!(vm_id = %vm_id, snapshot = %name, "Snapshot restored");
            Ok(())
        })
    }

    pub async fn delete_snapshot(&self, vm_id: &VmId, name: &str) -> Result<(), HypervisorError> {
        with_qmp!(self, vm_id, "snapshot delete", |qmp| {
            let cmd = format!("delvm {name}");
            qmp.human_monitor_command(&cmd).await.map_err(|e| {
                HypervisorError::ProcessError(format!("Failed to delete snapshot: {e}"))
            })?;
            tracing::info!(vm_id = %vm_id, snapshot = %name, "Snapshot deleted");
            Ok(())
        })
    }

    /// List VM snapshots using QEMU's `info snapshots` HMP command.
    ///
    /// Returns a vector of snapshot names.  Empty if none exist.
    pub async fn list_snapshots(&self, vm_id: &VmId) -> Result<Vec<String>, HypervisorError> {
        with_qmp!(self, vm_id, "snapshot list", |qmp| {
            let output = qmp
                .human_monitor_command("info snapshots")
                .await
                .map_err(|e| {
                    HypervisorError::ProcessError(format!("Failed to list snapshots: {e}"))
                })?;
            let names: Vec<String> = output
                .lines()
                .filter_map(|line| {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 && parts[0].parse::<u32>().is_ok() {
                        Some(parts[1].to_string())
                    } else {
                        None
                    }
                })
                .collect();
            Ok(names)
        })
    }
}
