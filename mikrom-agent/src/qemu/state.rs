use crate::hypervisor::{HypervisorError, VmInfo};
use crate::qemu::manager::QemuManager;
use mikrom_proto::id::VmId;
use std::path::PathBuf;

impl QemuManager {
    pub(crate) fn vm_state_path(&self, vm_id: &VmId) -> PathBuf {
        self.config.data_dir.join(format!("qemu-{vm_id}.json"))
    }

    pub async fn persist_runtime_state(&self) -> Result<(), HypervisorError> {
        let vms = self.vms.read().await;
        for (vm_id, vm_info) in vms.iter() {
            let path = self.vm_state_path(vm_id);
            let json = serde_json::to_string_pretty(vm_info).map_err(|e| {
                HypervisorError::ProcessError(format!("Failed to serialize VM state: {e}"))
            })?;
            tokio::fs::write(&path, json).await.map_err(|e| {
                HypervisorError::ProcessError(format!("Failed to write VM state: {e}"))
            })?;
        }
        Ok(())
    }

    pub async fn load_runtime_state(&self) -> Result<(), HypervisorError> {
        let data_dir = &self.config.data_dir;
        if !data_dir.exists() {
            return Ok(());
        }
        let mut entries = tokio::fs::read_dir(data_dir).await.map_err(|e| {
            HypervisorError::ProcessError(format!("Failed to read QEMU state dir: {e}"))
        })?;
        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            HypervisorError::ProcessError(format!("Failed to read state entry: {e}"))
        })? {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                let content = tokio::fs::read_to_string(&path).await.ok();
                if let Some(json) = content
                    && let Ok(vm_info) = serde_json::from_str::<VmInfo>(&json)
                {
                    self.vms.write().await.insert(vm_info.vm_id, vm_info);
                }
            }
        }
        Ok(())
    }
}
