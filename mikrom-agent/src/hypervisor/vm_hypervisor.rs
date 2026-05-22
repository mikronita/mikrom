use crate::hypervisor::error::HypervisorError;
use crate::hypervisor::types::{VmConfig, VmDetailedInfo, VmInfo};
use mikrom_proto::id::{AppId, VmId};
use std::fmt;

pub use async_trait::async_trait;

/// Supported hypervisor types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HypervisorType {
    Firecracker,
    QemuMicrovm,
}

impl HypervisorType {
    pub fn as_str(&self) -> &'static str {
        match self {
            HypervisorType::Firecracker => "firecracker",
            HypervisorType::QemuMicrovm => "qemu-microvm",
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
}
