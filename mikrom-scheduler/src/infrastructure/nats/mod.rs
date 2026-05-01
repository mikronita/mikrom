pub mod event_loop;

pub use event_loop::NatsEventLoop;

use crate::domain::{AgentClient, DomainError, DomainResult, VmConfig};
use async_trait::async_trait;
use mikrom_proto::agent::{
    AgentCommand, AgentCommandResponse, DeleteVmRequest, PauseVmRequest, ResumeVmRequest,
    StartVmRequest, StopVmRequest, VmConfig as ProtoVmConfig,
};
use prost::Message;
use std::time::Duration;

pub struct NatsAgentClient {
    client: async_nats::Client,
}

impl NatsAgentClient {
    pub fn new(client: async_nats::Client) -> Self {
        Self { client }
    }

    async fn send_command(
        &self,
        host_id: &str,
        command: mikrom_proto::agent::agent_command::Command,
    ) -> DomainResult<()> {
        let subject = format!("mikrom.agent.{}.cmd", host_id);
        let cmd = AgentCommand {
            command: Some(command),
        };

        let mut payload = Vec::new();
        cmd.encode(&mut payload)
            .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

        let response = tokio::time::timeout(
            Duration::from_secs(15),
            self.client.request(subject, payload.into()),
        )
        .await
        .map_err(|_| DomainError::Infrastructure("Agent request timed out".to_string()))?
        .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

        let inner = AgentCommandResponse::decode(&response.payload[..]).map_err(|e| {
            DomainError::Infrastructure(format!("Failed to decode agent response: {}", e))
        })?;

        if inner.success {
            Ok(())
        } else {
            Err(DomainError::Infrastructure(inner.message))
        }
    }
}

#[async_trait]
impl AgentClient for NatsAgentClient {
    async fn start_vm(
        &self,
        host_id: &str,
        app_id: &str,
        image: &str,
        vm_id: &str,
        config: &VmConfig,
    ) -> DomainResult<()> {
        let proto_config = ProtoVmConfig {
            vcpus: config.vcpus,
            memory_mib: config.memory_mib as u32,
            disk_mib: config.disk_mib as u32,
            port: config.port,
            env: config.env.clone(),
            ip_address: config.ip_address.clone().unwrap_or_default(),
            gateway: config.gateway.clone().unwrap_or_default(),
            mac_address: config.mac_address.clone().unwrap_or_default(),
            netmask: config.netmask.clone().unwrap_or_default(),
            volumes: vec![], // TODO
        };

        self.send_command(
            host_id,
            mikrom_proto::agent::agent_command::Command::StartVm(StartVmRequest {
                vm_id: vm_id.to_string(),
                app_id: app_id.to_string(),
                image: image.to_string(),
                config: Some(proto_config),
            }),
        )
        .await
    }

    async fn pause_vm(&self, host_id: &str, vm_id: &str) -> DomainResult<()> {
        self.send_command(
            host_id,
            mikrom_proto::agent::agent_command::Command::PauseVm(PauseVmRequest {
                vm_id: vm_id.to_string(),
            }),
        )
        .await
    }

    async fn resume_vm(&self, host_id: &str, vm_id: &str) -> DomainResult<()> {
        self.send_command(
            host_id,
            mikrom_proto::agent::agent_command::Command::ResumeVm(ResumeVmRequest {
                vm_id: vm_id.to_string(),
            }),
        )
        .await
    }

    async fn stop_vm(&self, host_id: &str, vm_id: &str) -> DomainResult<()> {
        self.send_command(
            host_id,
            mikrom_proto::agent::agent_command::Command::StopVm(StopVmRequest {
                vm_id: vm_id.to_string(),
            }),
        )
        .await
    }

    async fn delete_vm(&self, host_id: &str, vm_id: &str) -> DomainResult<()> {
        self.send_command(
            host_id,
            mikrom_proto::agent::agent_command::Command::DeleteVm(DeleteVmRequest {
                vm_id: vm_id.to_string(),
            }),
        )
        .await
    }
}
