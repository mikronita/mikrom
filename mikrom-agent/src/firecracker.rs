use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FirecrackerError {
    #[error("VM not found: {0}")]
    VmNotFound(String),
    #[error("Failed to start VM: {0}")]
    StartFailed(String),
    #[error("Failed to stop VM: {0}")]
    StopFailed(String),
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VmStatus {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
}

impl Default for VmStatus {
    fn default() -> Self {
        Self::Stopped
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VmInfo {
    pub vm_id: String,
    pub app_id: String,
    pub image: String,
    pub config: VmConfig,
    pub status: VmStatus,
    pub started_at: Option<i64>,
    pub error_message: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VmConfig {
    pub vcpus: u32,
    pub memory_mib: u64,
    pub disk_mib: u64,
    pub env: std::collections::HashMap<String, String>,
}

#[derive(Clone)]
pub struct FirecrackerManager {
    vms: Arc<RwLock<HashMap<String, VmInfo>>>,
}

impl FirecrackerManager {
    pub fn new() -> Self {
        Self {
            vms: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn start_vm(
        &self,
        vm_id: String,
        app_id: String,
        image: String,
        config: VmConfig,
    ) -> Result<(), FirecrackerError> {
        let mut vms = self.vms.write();

        if vms.contains_key(&vm_id) {
            return Err(FirecrackerError::StartFailed(
                "VM already exists".to_string(),
            ));
        }

        let vm_info = VmInfo {
            vm_id: vm_id.clone(),
            app_id,
            image,
            config,
            status: VmStatus::Starting,
            started_at: None,
            error_message: None,
        };

        vms.insert(vm_id, vm_info);

        Ok(())
    }

    pub fn stop_vm(&self, vm_id: &str) -> Result<(), FirecrackerError> {
        let mut vms = self.vms.write();

        match vms.get_mut(vm_id) {
            Some(vm) => {
                vm.status = VmStatus::Stopping;
                Ok(())
            }
            None => Err(FirecrackerError::VmNotFound(vm_id.to_string())),
        }
    }

    pub fn get_vm_status(&self, vm_id: &str) -> Result<VmStatus, FirecrackerError> {
        let vms = self.vms.read();

        match vms.get(vm_id) {
            Some(vm) => Ok(vm.status),
            None => Err(FirecrackerError::VmNotFound(vm_id.to_string())),
        }
    }

    pub fn list_vms(&self) -> Vec<VmInfo> {
        self.vms.read().values().cloned().collect()
    }

    pub fn get_vm(&self, vm_id: &str) -> Option<VmInfo> {
        self.vms.read().get(vm_id).cloned()
    }
}

impl Default for FirecrackerManager {
    fn default() -> Self {
        Self::new()
    }
}
