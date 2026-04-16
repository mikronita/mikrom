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

#[derive(Clone, Debug, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum VmStatus {
    Starting,
    Running,
    Stopping,
    #[default]
    Stopped,
    Failed,
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

#[cfg(test)]
impl FirecrackerManager {
    pub fn set_status_for_test(&self, vm_id: &str, status: VmStatus) {
        if let Some(vm) = self.vms.write().get_mut(vm_id) {
            vm.status = status;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> VmConfig {
        VmConfig {
            vcpus: 1,
            memory_mib: 256,
            disk_mib: 1024,
            env: Default::default(),
        }
    }

    fn start(mgr: &FirecrackerManager, vm_id: &str) {
        mgr.start_vm(
            vm_id.to_string(),
            "app-1".to_string(),
            "nginx:latest".to_string(),
            config(),
        )
        .unwrap();
    }

    #[test]
    fn test_start_vm_succeeds() {
        let mgr = FirecrackerManager::new();
        assert!(
            mgr.start_vm(
                "vm-1".to_string(),
                "app-1".to_string(),
                "img".to_string(),
                config()
            )
            .is_ok()
        );
    }

    #[test]
    fn test_started_vm_has_starting_status() {
        let mgr = FirecrackerManager::new();
        start(&mgr, "vm-1");
        assert_eq!(mgr.get_vm_status("vm-1").unwrap(), VmStatus::Starting);
    }

    #[test]
    fn test_start_duplicate_vm_fails() {
        let mgr = FirecrackerManager::new();
        start(&mgr, "vm-1");
        let result = mgr.start_vm(
            "vm-1".to_string(),
            "app-1".to_string(),
            "img".to_string(),
            config(),
        );
        assert!(matches!(result, Err(FirecrackerError::StartFailed(_))));
    }

    #[test]
    fn test_stop_vm_transitions_to_stopping() {
        let mgr = FirecrackerManager::new();
        start(&mgr, "vm-1");
        assert!(mgr.stop_vm("vm-1").is_ok());
        assert_eq!(mgr.get_vm_status("vm-1").unwrap(), VmStatus::Stopping);
    }

    #[test]
    fn test_stop_nonexistent_vm_returns_error() {
        let mgr = FirecrackerManager::new();
        assert!(matches!(
            mgr.stop_vm("ghost"),
            Err(FirecrackerError::VmNotFound(_))
        ));
    }

    #[test]
    fn test_get_status_nonexistent_returns_error() {
        let mgr = FirecrackerManager::new();
        assert!(matches!(
            mgr.get_vm_status("ghost"),
            Err(FirecrackerError::VmNotFound(_))
        ));
    }

    #[test]
    fn test_list_vms_empty() {
        assert!(FirecrackerManager::new().list_vms().is_empty());
    }

    #[test]
    fn test_list_vms_after_starts() {
        let mgr = FirecrackerManager::new();
        start(&mgr, "vm-1");
        start(&mgr, "vm-2");
        assert_eq!(mgr.list_vms().len(), 2);
    }

    #[test]
    fn test_get_vm_returns_correct_info() {
        let mgr = FirecrackerManager::new();
        mgr.start_vm(
            "vm-1".to_string(),
            "app-42".to_string(),
            "ubuntu:24.04".to_string(),
            config(),
        )
        .unwrap();
        let vm = mgr.get_vm("vm-1").unwrap();
        assert_eq!(vm.app_id, "app-42");
        assert_eq!(vm.image, "ubuntu:24.04");
        assert_eq!(vm.config.vcpus, 1);
        assert_eq!(vm.config.memory_mib, 256);
    }

    #[test]
    fn test_get_vm_nonexistent_returns_none() {
        assert!(FirecrackerManager::new().get_vm("ghost").is_none());
    }

    #[test]
    fn test_vm_status_default_is_stopped() {
        assert_eq!(VmStatus::default(), VmStatus::Stopped);
    }

    #[test]
    fn test_vm_info_serialization_roundtrip() {
        let mgr = FirecrackerManager::new();
        start(&mgr, "vm-1");
        let vm = mgr.get_vm("vm-1").unwrap();
        let json = serde_json::to_string(&vm).unwrap();
        let restored: VmInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.vm_id, "vm-1");
        assert_eq!(restored.status, VmStatus::Starting);
    }

    #[test]
    fn test_vm_config_with_env_vars() {
        let mut env = std::collections::HashMap::new();
        env.insert("PORT".to_string(), "3000".to_string());
        env.insert("ENV".to_string(), "prod".to_string());
        let cfg = VmConfig {
            vcpus: 2,
            memory_mib: 512,
            disk_mib: 2048,
            env,
        };
        assert_eq!(cfg.env.get("PORT").unwrap(), "3000");
        assert_eq!(cfg.vcpus, 2);
    }

    #[test]
    fn test_error_messages_contain_vm_id() {
        let err = FirecrackerError::VmNotFound("vm-99".to_string());
        assert!(err.to_string().contains("vm-99"));
        let err2 = FirecrackerError::StartFailed("already exists".to_string());
        assert!(err2.to_string().contains("already exists"));
        let err3 = FirecrackerError::StopFailed("busy".to_string());
        assert!(err3.to_string().contains("busy"));
    }

    #[test]
    fn test_set_status_for_test_to_running() {
        let mgr = FirecrackerManager::new();
        start(&mgr, "vm-1");
        mgr.set_status_for_test("vm-1", VmStatus::Running);
        assert_eq!(mgr.get_vm_status("vm-1").unwrap(), VmStatus::Running);
    }

    #[test]
    fn test_set_status_for_test_to_stopped() {
        let mgr = FirecrackerManager::new();
        start(&mgr, "vm-1");
        mgr.set_status_for_test("vm-1", VmStatus::Stopped);
        assert_eq!(mgr.get_vm_status("vm-1").unwrap(), VmStatus::Stopped);
    }

    #[test]
    fn test_set_status_for_test_to_failed() {
        let mgr = FirecrackerManager::new();
        start(&mgr, "vm-1");
        mgr.set_status_for_test("vm-1", VmStatus::Failed);
        assert_eq!(mgr.get_vm_status("vm-1").unwrap(), VmStatus::Failed);
    }

    #[test]
    fn test_set_status_for_test_on_nonexistent_vm_is_noop() {
        let mgr = FirecrackerManager::new();
        // Must not panic
        mgr.set_status_for_test("ghost", VmStatus::Running);
    }

    #[test]
    fn test_manager_is_cloneable_and_shares_state() {
        let mgr = FirecrackerManager::new();
        start(&mgr, "vm-1");
        let cloned = mgr.clone();
        // Cloned manager sees the same VMs (Arc is shared)
        assert_eq!(cloned.get_vm_status("vm-1").unwrap(), VmStatus::Starting);
        cloned.set_status_for_test("vm-1", VmStatus::Running);
        assert_eq!(mgr.get_vm_status("vm-1").unwrap(), VmStatus::Running);
    }
}
