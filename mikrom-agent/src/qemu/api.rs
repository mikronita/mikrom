use crate::hypervisor::{VmDetailedInfo, VmInfo, VmStatus};
use crate::qemu::manager::QemuManager;
use mikrom_proto::id::VmId;

impl QemuManager {
    pub async fn get_vm_info(&self, vm_id: &VmId) -> Option<VmInfo> {
        let vms = self.vms.read().await;
        vms.get(vm_id).cloned()
    }

    pub async fn get_all_vms(&self) -> Vec<VmDetailedInfo> {
        let vms = self.vms.read().await;
        let procs = self.processes.lock().await;
        vms.values()
            .map(|vm| {
                let proc = procs.get(&vm.vm_id);
                VmDetailedInfo {
                    vm_id: vm.vm_id,
                    app_id: vm.app_id,
                    status: vm.status,
                    error_message: vm.error_message.clone(),
                    pid: proc.map(|p| p.pid),
                    metrics_path: None,
                    socket_path: proc.map(|p| p.qmp_socket.to_string_lossy().to_string()),
                    tap_name: proc.map(|p| p.tap_name.clone()),
                    tap_ifindex: None,
                }
            })
            .collect()
    }

    pub async fn get_vm_started_at_ms(&self, vm_id: &VmId) -> Option<u64> {
        let vms = self.vms.read().await;
        vms.get(vm_id)
            .and_then(|vm| vm.started_at.map(|t| t as u64 * 1000))
    }

    pub async fn is_app_started(&self, vm_id: &VmId) -> bool {
        let vms = self.vms.read().await;
        vms.get(vm_id)
            .map_or(false, |vm| vm.status == VmStatus::Running)
    }
}
