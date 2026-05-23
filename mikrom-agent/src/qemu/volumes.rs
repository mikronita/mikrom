use crate::hypervisor::HypervisorError;
use crate::qemu::manager::QemuManager;
use mikrom_proto::id::VmId;
use std::path::Path;
use std::time::Duration;

impl QemuManager {
    async fn qmp_best_effort(
        &self,
        vm_id: &VmId,
        context: &'static str,
        qmp: &mut crate::qemu::QmpClient,
        command: &str,
        args: serde_json::Value,
    ) {
        if let Err(e) = qmp.execute_with_args(command, args).await {
            tracing::debug!(vm_id = %vm_id, error = %e, %context, "Best-effort QMP operation failed");
        }
    }

    /// Hot-plug a virtio-blk volume into a running VM.
    pub async fn attach_volume(
        &self,
        vm_id: &VmId,
        volume_id: &str,
        image_path: &Path,
        read_only: bool,
    ) -> Result<(), HypervisorError> {
        let node_name = format!("vol-{volume_id}");
        let device_id = format!("virtio-blk-{volume_id}");
        let path_str = image_path.to_string_lossy();

        with_qmp!(self, vm_id, "hot-plug", |qmp| {
            let blockdev_args = serde_json::json!({
                "driver": "file",
                "filename": path_str.as_ref(),
                "node-name": node_name,
                "read-only": read_only,
            });
            qmp.execute_with_args("blockdev-add", blockdev_args)
                .await
                .map_err(|e| HypervisorError::ProcessError(format!("blockdev-add failed: {e}")))?;

            let device_args = serde_json::json!({
                "driver": "virtio-blk-device",
                "drive": node_name,
                "id": device_id,
            });
            qmp.execute_with_args("device_add", device_args)
                .await
                .map_err(|e| HypervisorError::ProcessError(format!("device_add failed: {e}")))?;

            tracing::info!(vm_id = %vm_id, volume = %volume_id, path = %path_str, read_only = read_only, "Volume hot-plugged");
            Ok(())
        })
    }

    /// Hot-unplug a virtio-blk volume from a running VM.
    pub async fn detach_volume(
        &self,
        vm_id: &VmId,
        volume_id: &str,
    ) -> Result<(), HypervisorError> {
        let node_name = format!("vol-{volume_id}");
        let device_id = format!("virtio-blk-{volume_id}");

        with_qmp!(self, vm_id, "hot-unplug", |qmp| {
            let del_args = serde_json::json!({ "id": device_id });
            self.qmp_best_effort(
                vm_id,
                "qemu-detach-device-del",
                &mut qmp,
                "device_del",
                del_args,
            )
            .await;

            tokio::time::sleep(Duration::from_millis(200)).await;

            let blockdel_args = serde_json::json!({ "node-name": node_name });
            self.qmp_best_effort(
                vm_id,
                "qemu-detach-blockdev-del",
                &mut qmp,
                "blockdev-del",
                blockdel_args,
            )
            .await;

            tracing::info!(vm_id = %vm_id, volume = %volume_id, "Volume hot-unplugged");
            Ok(())
        })
    }
}
