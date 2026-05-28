use crate::hypervisor::error::HypervisorError;
use crate::hypervisor::types::{VmConfig, VmDetailedInfo, VmInfo};
use mikrom_proto::id::{AppId, VmId};
use std::fmt;

pub use async_trait::async_trait;

/// Supported hypervisor types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HypervisorType {
    Firecracker = 1,
    QemuMicrovm = 2,
    CloudHypervisor = 3,
}

impl HypervisorType {
    pub fn as_str(&self) -> &'static str {
        match self {
            HypervisorType::Firecracker => "firecracker",
            HypervisorType::QemuMicrovm => "qemu-microvm",
            HypervisorType::CloudHypervisor => "cloud-hypervisor",
        }
    }
}

impl fmt::Display for HypervisorType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Trait that every hypervisor implementation must satisfy.
///
/// This is the central abstraction that allows Mikrom to support multiple
/// VMMs (Firecracker, QEMU microvm, etc.) on the same agent host.
#[async_trait]
pub trait VmHypervisor: Send + Sync + fmt::Debug {
    fn hypervisor_type(&self) -> HypervisorType;
    fn agent_id(&self) -> &str;

    // ── VM lifecycle ──────────────────────────────────────────

    async fn start_vm(
        &self,
        vm_id: VmId,
        app_id: AppId,
        image: String,
        config: VmConfig,
    ) -> Result<(), HypervisorError>;

    async fn stop_vm(&self, vm_id: &VmId) -> Result<(), HypervisorError>;

    async fn pause_vm(&self, vm_id: &VmId) -> Result<(), HypervisorError>;

    async fn resume_vm(&self, vm_id: &VmId) -> Result<(), HypervisorError>;

    async fn delete_vm(&self, vm_id: &VmId) -> Result<(), HypervisorError>;

    async fn restart_vm(&self, vm_id: &VmId) -> Result<(), HypervisorError>;

    // ── Query ─────────────────────────────────────────────────

    async fn get_vm_info(&self, vm_id: &VmId) -> Option<VmInfo>;

    async fn get_all_vms(&self) -> Vec<VmDetailedInfo>;

    async fn get_vm_started_at_ms(&self, vm_id: &VmId) -> Option<u64>;

    async fn is_app_started(&self, vm_id: &VmId) -> bool;

    fn get_logs(&self, vm_id: &VmId) -> Vec<String>;

    // ── Firewall ──────────────────────────────────────────────

    async fn update_vm_firewall(
        &self,
        vm_id: &VmId,
        rules: Vec<mikrom_agent_ebpf_common::FirewallRule>,
    ) -> Result<(), HypervisorError>;

    // ── Host-level lifecycle ───────────────────────────────────

    async fn init_network(&self) -> Result<(), HypervisorError>;

    async fn load_runtime_state(&self) -> Result<(), HypervisorError>;

    async fn persist_runtime_state(&self) -> Result<(), HypervisorError>;

    async fn cleanup_all_stale_resources(&self);

    async fn set_nats_client(&self, client: async_nats::Client);

    fn start_background_tasks(&self);

    // ── VM Snapshots ──────────────────────────────────────────

    async fn create_vm_snapshot(&self, _vm_id: &VmId, _name: &str) -> Result<(), HypervisorError> {
        Err(HypervisorError::UnsupportedOperation(
            "VM snapshots not supported".to_string(),
        ))
    }

    async fn restore_vm_snapshot(&self, _vm_id: &VmId, _name: &str) -> Result<(), HypervisorError> {
        Err(HypervisorError::UnsupportedOperation(
            "VM snapshot restore not supported".to_string(),
        ))
    }

    async fn delete_vm_snapshot(&self, _vm_id: &VmId, _name: &str) -> Result<(), HypervisorError> {
        Err(HypervisorError::UnsupportedOperation(
            "VM snapshot delete not supported".to_string(),
        ))
    }

    async fn list_vm_snapshots(
        &self,
        _vm_id: &VmId,
    ) -> Result<Vec<mikrom_proto::agent::VmSnapshotInfo>, HypervisorError> {
        Err(HypervisorError::UnsupportedOperation(
            "VM snapshot list not supported".to_string(),
        ))
    }

    // ── Volume Hot-Plug ───────────────────────────────────────

    async fn attach_volume(
        &self,
        _vm_id: &VmId,
        _volume_id: &str,
        _mount_point: &str,
        _read_only: bool,
    ) -> Result<(), HypervisorError> {
        Err(HypervisorError::UnsupportedOperation(
            "Volume attach not supported".to_string(),
        ))
    }

    async fn detach_volume(&self, _vm_id: &VmId, _volume_id: &str) -> Result<(), HypervisorError> {
        Err(HypervisorError::UnsupportedOperation(
            "Volume detach not supported".to_string(),
        ))
    }

    // ── Live Migration ────────────────────────────────────────

    async fn start_migration(
        &self,
        _vm_id: &VmId,
        _target_host: &str,
        _target_uri: &str,
    ) -> Result<(), HypervisorError> {
        Err(HypervisorError::UnsupportedOperation(
            "Live migration not supported".to_string(),
        ))
    }

    async fn cancel_migration(&self, _vm_id: &VmId) -> Result<(), HypervisorError> {
        Err(HypervisorError::UnsupportedOperation(
            "Migration cancel not supported".to_string(),
        ))
    }

    async fn query_migration(&self, _vm_id: &VmId) -> Result<String, HypervisorError> {
        Err(HypervisorError::UnsupportedOperation(
            "Migration query not supported".to_string(),
        ))
    }

    // ── Balloon ────────────────────────────────────────────────

    async fn set_balloon_size(
        &self,
        _vm_id: &VmId,
        _target_memory_mib: u32,
    ) -> Result<(), HypervisorError> {
        Err(HypervisorError::UnsupportedOperation(
            "Balloon not supported".to_string(),
        ))
    }

    async fn query_balloon(&self, _vm_id: &VmId) -> Result<(u32, u32), HypervisorError> {
        Err(HypervisorError::UnsupportedOperation(
            "Balloon query not supported".to_string(),
        ))
    }
}
