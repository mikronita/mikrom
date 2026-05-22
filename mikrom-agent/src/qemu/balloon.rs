use crate::hypervisor::HypervisorError;
use crate::qemu::manager::QemuManager;
use mikrom_proto::id::VmId;

impl QemuManager {
    /// Set the balloon target size for a running VM.
    pub async fn set_balloon_size(
        &self,
        vm_id: &VmId,
        size_mb: u64,
    ) -> Result<(), HypervisorError> {
        with_qmp!(self, vm_id, "balloon", |qmp| {
            let args = serde_json::json!({ "value": size_mb });
            qmp.execute_with_args("balloon", args)
                .await
                .map_err(|e| HypervisorError::ProcessError(format!("balloon failed: {e}")))?;
            tracing::info!(vm_id = %vm_id, size_mb = size_mb, "Balloon size set");
            Ok(())
        })
    }

    /// Query the current balloon statistics for a VM.
    pub async fn query_balloon(&self, vm_id: &VmId) -> Result<(u64, u64), HypervisorError> {
        with_qmp!(self, vm_id, "balloon query", |qmp| {
            let resp = qmp
                .execute("query-balloon")
                .await
                .map_err(|e| HypervisorError::ProcessError(format!("query-balloon failed: {e}")))?;
            let ret = resp.get("return").ok_or_else(|| {
                HypervisorError::ProcessError("Invalid query-balloon response".to_string())
            })?;
            let actual = ret.get("actual").and_then(|v| v.as_u64()).unwrap_or(0);
            let max = ret.get("max").and_then(|v| v.as_u64()).unwrap_or(0);
            Ok((actual / (1024 * 1024), max / (1024 * 1024)))
        })
    }
}
