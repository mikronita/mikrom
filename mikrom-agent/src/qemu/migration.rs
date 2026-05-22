use crate::hypervisor::HypervisorError;
use crate::qemu::manager::QemuManager;
use mikrom_proto::id::VmId;

impl QemuManager {
    /// Start live migration of a running VM to a destination URI.
    pub async fn start_migration(
        &self,
        vm_id: &VmId,
        destination_uri: &str,
    ) -> Result<(), HypervisorError> {
        with_qmp!(self, vm_id, "migration", |qmp| {
            let args = serde_json::json!({ "uri": destination_uri });
            qmp.execute_with_args("migrate", args)
                .await
                .map_err(|e| HypervisorError::ProcessError(format!("migrate failed: {e}")))?;
            tracing::info!(vm_id = %vm_id, destination = %destination_uri, "Live migration started");
            Ok(())
        })
    }

    /// Cancel an ongoing live migration.
    pub async fn cancel_migration(&self, vm_id: &VmId) -> Result<(), HypervisorError> {
        with_qmp!(self, vm_id, "migration cancel", |qmp| {
            qmp.execute("migrate_cancel").await.map_err(|e| {
                HypervisorError::ProcessError(format!("migrate_cancel failed: {e}"))
            })?;
            tracing::info!(vm_id = %vm_id, "Live migration cancelled");
            Ok(())
        })
    }

    /// Query the status of an ongoing or completed live migration.
    pub async fn query_migration(&self, vm_id: &VmId) -> Result<String, HypervisorError> {
        with_qmp!(self, vm_id, "migration query", |qmp| {
            let resp = qmp
                .execute("query-migrate")
                .await
                .map_err(|e| HypervisorError::ProcessError(format!("query-migrate failed: {e}")))?;
            let status = resp
                .get("return")
                .and_then(|v| v.get("status"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            Ok(status.to_string())
        })
    }
}
