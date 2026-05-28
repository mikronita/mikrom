use crate::cloud_hypervisor::CloudHypervisorManager;
use crate::config::AgentConfig;
use crate::firecracker::FirecrackerManager;
use crate::hypervisor::vm_hypervisor::{HypervisorType, VmHypervisor};
use std::collections::HashMap;
use std::sync::Arc;

/// Build the set of hypervisors enabled for this agent.
///
/// Each entry in the returned map corresponds to a VMM that the agent
/// can use to run microVMs.  The scheduler learns about available
/// hypervisors via the `supported_hypervisors` field in the heartbeat.
pub async fn create_hypervisors(
    config: &AgentConfig,
) -> HashMap<HypervisorType, Arc<dyn VmHypervisor>> {
    let mut hvs: HashMap<HypervisorType, Arc<dyn VmHypervisor>> = HashMap::new();

    // Firecracker is always enabled.
    hvs.insert(
        HypervisorType::Firecracker,
        Arc::new(FirecrackerManager::new().await),
    );

    if config.cloud_hypervisor_enabled {
        hvs.insert(
            HypervisorType::CloudHypervisor,
            Arc::new(CloudHypervisorManager::new(config.clone()).await),
        );
    }

    hvs
}
