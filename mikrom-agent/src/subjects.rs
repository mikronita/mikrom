//! Local NATS subject helpers for the agent.

use mikrom_proto::subjects::SharedSubject;

pub fn mesh_updates(host_id: &str) -> String {
    format!("mikrom.scheduler.network.mesh.{host_id}")
}

pub fn agent_command(host_id: &str) -> String {
    format!("mikrom.agent.{host_id}.cmd")
}

pub fn agent_health_check(host_id: &str) -> String {
    format!("mikrom.agent.{host_id}.check_health")
}

pub const SCHEDULER_WORKER_HEARTBEAT: &str = SharedSubject::SchedulerWorkerHeartbeat.as_str();
pub const SCHEDULER_VM_FAILED: &str = SharedSubject::SchedulerVmFailed.as_str();
