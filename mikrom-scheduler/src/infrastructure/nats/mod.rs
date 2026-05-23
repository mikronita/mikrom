pub mod event_loop;
pub mod subjects;

pub use event_loop::NatsEventLoop;

use crate::domain::{AgentClient, DomainError, DomainResult, VmConfig};
use async_trait::async_trait;
use mikrom_proto::agent::{
    AgentCommand, AgentCommandResponse, AttachVolumeRequest, CancelMigrationRequest,
    DeleteVmRequest, DetachVolumeRequest, PauseVmRequest, QueryBalloonRequest,
    QueryBalloonResponse, QueryMigrationRequest, QueryMigrationResponse, RestoreSnapshotRequest,
    ResumeVmRequest, SetBalloonRequest, StartMigrationRequest, StartVmRequest, StopVmRequest,
    UpdateFirewallRequest, VmConfig as ProtoVmConfig, VmSnapshotCreateRequest,
    VmSnapshotDeleteRequest, VmSnapshotListRequest, VmSnapshotListResponse,
    VmSnapshotRestoreRequest, Volume as ProtoVolume,
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
        tracing::debug!(?command, %subject, "Sending command to agent");
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

    async fn send_command_raw(
        &self,
        host_id: &str,
        command: mikrom_proto::agent::agent_command::Command,
    ) -> DomainResult<Vec<u8>> {
        let subject = format!("mikrom.agent.{}.cmd", host_id);
        tracing::debug!(?command, %subject, "Sending command to agent (raw)");
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

        Ok(response.payload.to_vec())
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
            volumes: config
                .volumes
                .iter()
                .map(|v| ProtoVolume {
                    volume_id: v.volume_id.to_string(),
                    size_mib: v.size_mib,
                    read_only: v.read_only,
                    pool_name: v.pool_name.clone(),
                    mount_point: v.mount_point.clone(),
                    access_mode: v.access_mode as i32,
                })
                .collect(),
            health_check_path: config.health_check_path.clone(),
            ipv6_address: config.ipv6_address.clone().unwrap_or_default(),
            ipv6_gateway: config.ipv6_gateway.clone().unwrap_or_default(),
            hypervisor: config.hypervisor as i32, // HypervisorType enum discriminant matches proto
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

    async fn check_health(&self, host_id: &str, vm_id: &str) -> DomainResult<bool> {
        let subject = format!("mikrom.agent.{host_id}.check_health");
        let req = mikrom_proto::agent::CheckHealthRequest {
            vm_id: vm_id.to_string(),
        };

        let mut payload = Vec::new();
        req.encode(&mut payload)
            .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

        let response = tokio::time::timeout(
            Duration::from_secs(2),
            self.client.request(subject, payload.into()),
        )
        .await
        .map_err(|_| DomainError::Infrastructure("Health check timed out".to_string()))?
        .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

        let res = mikrom_proto::agent::CheckHealthResponse::decode(&response.payload[..])
            .map_err(|e| DomainError::Infrastructure(format!("Decode failed: {e}")))?;

        if res.is_healthy {
            Ok(true)
        } else {
            Err(DomainError::Infrastructure(if res.message.is_empty() {
                "Unhealthy".to_string()
            } else {
                res.message
            }))
        }
    }

    async fn update_firewall(
        &self,
        host_id: &str,
        vm_id: &str,
        rules: Vec<mikrom_proto::scheduler::FirewallRule>,
    ) -> DomainResult<()> {
        let proto_rules = rules
            .into_iter()
            .map(|r| mikrom_proto::agent::FirewallRule {
                protocol: r.protocol,
                port_start: r.port_start,
                port_end: r.port_end,
                action: r.action,
            })
            .collect();

        self.send_command(
            host_id,
            mikrom_proto::agent::agent_command::Command::UpdateFirewall(UpdateFirewallRequest {
                vm_id: vm_id.to_string(),
                rules: proto_rules,
            }),
        )
        .await
    }

    async fn create_volume(
        &self,
        host_id: &str,
        volume_id: &str,
        size_mib: u32,
        pool_name: &str,
    ) -> DomainResult<()> {
        self.send_command(
            host_id,
            mikrom_proto::agent::agent_command::Command::CreateVolume(
                mikrom_proto::agent::CreateVolumeRequest {
                    volume_id: volume_id.to_string(),
                    size_mib,
                    pool_name: pool_name.to_string(),
                },
            ),
        )
        .await
    }

    async fn create_snapshot(
        &self,
        host_id: &str,
        volume_id: &str,
        snapshot_name: &str,
        pool_name: &str,
    ) -> DomainResult<()> {
        self.send_command(
            host_id,
            mikrom_proto::agent::agent_command::Command::CreateSnapshot(
                mikrom_proto::agent::CreateSnapshotRequest {
                    volume_id: volume_id.to_string(),
                    snapshot_name: snapshot_name.to_string(),
                    pool_name: pool_name.to_string(),
                },
            ),
        )
        .await
    }

    async fn delete_volume(
        &self,
        host_id: &str,
        volume_id: &str,
        pool_name: &str,
    ) -> DomainResult<()> {
        self.send_command(
            host_id,
            mikrom_proto::agent::agent_command::Command::DeleteVolume(
                mikrom_proto::agent::DeleteVolumeRequest {
                    volume_id: volume_id.to_string(),
                    pool_name: pool_name.to_string(),
                },
            ),
        )
        .await
    }

    async fn delete_snapshot(
        &self,
        host_id: &str,
        volume_id: &str,
        snapshot_name: &str,
        pool_name: &str,
    ) -> DomainResult<()> {
        self.send_command(
            host_id,
            mikrom_proto::agent::agent_command::Command::DeleteSnapshot(
                mikrom_proto::agent::DeleteSnapshotRequest {
                    volume_id: volume_id.to_string(),
                    snapshot_name: snapshot_name.to_string(),
                    pool_name: pool_name.to_string(),
                },
            ),
        )
        .await
    }

    async fn restore_snapshot(
        &self,
        host_id: &str,
        volume_id: &str,
        snapshot_name: &str,
        pool_name: &str,
    ) -> DomainResult<()> {
        self.send_command(
            host_id,
            mikrom_proto::agent::agent_command::Command::RestoreSnapshot(RestoreSnapshotRequest {
                volume_id: volume_id.to_string(),
                snapshot_name: snapshot_name.to_string(),
                pool_name: pool_name.to_string(),
            }),
        )
        .await
    }

    async fn clone_volume(
        &self,
        host_id: &str,
        source_volume_id: &str,
        snapshot_name: &str,
        target_volume_id: &str,
        pool_name: &str,
    ) -> DomainResult<()> {
        self.send_command(
            host_id,
            mikrom_proto::agent::agent_command::Command::CloneVolume(
                mikrom_proto::agent::CloneVolumeRequest {
                    source_volume_id: source_volume_id.to_string(),
                    snapshot_name: snapshot_name.to_string(),
                    target_volume_id: target_volume_id.to_string(),
                    pool_name: pool_name.to_string(),
                },
            ),
        )
        .await
    }

    async fn vm_snapshot_create(
        &self,
        host_id: &str,
        vm_id: &str,
        snapshot_name: &str,
    ) -> DomainResult<()> {
        self.send_command(
            host_id,
            mikrom_proto::agent::agent_command::Command::VmSnapshotCreate(
                VmSnapshotCreateRequest {
                    vm_id: vm_id.to_string(),
                    snapshot_name: snapshot_name.to_string(),
                },
            ),
        )
        .await
    }

    async fn vm_snapshot_restore(
        &self,
        host_id: &str,
        vm_id: &str,
        snapshot_name: &str,
    ) -> DomainResult<()> {
        self.send_command(
            host_id,
            mikrom_proto::agent::agent_command::Command::VmSnapshotRestore(
                VmSnapshotRestoreRequest {
                    vm_id: vm_id.to_string(),
                    snapshot_name: snapshot_name.to_string(),
                },
            ),
        )
        .await
    }

    async fn vm_snapshot_delete(
        &self,
        host_id: &str,
        vm_id: &str,
        snapshot_name: &str,
    ) -> DomainResult<()> {
        self.send_command(
            host_id,
            mikrom_proto::agent::agent_command::Command::VmSnapshotDelete(
                VmSnapshotDeleteRequest {
                    vm_id: vm_id.to_string(),
                    snapshot_name: snapshot_name.to_string(),
                },
            ),
        )
        .await
    }

    async fn vm_snapshot_list(
        &self,
        host_id: &str,
        vm_id: &str,
    ) -> DomainResult<Vec<mikrom_proto::agent::VmSnapshotInfo>> {
        let bytes = self
            .send_command_raw(
                host_id,
                mikrom_proto::agent::agent_command::Command::VmSnapshotList(
                    VmSnapshotListRequest {
                        vm_id: vm_id.to_string(),
                    },
                ),
            )
            .await?;
        let resp = VmSnapshotListResponse::decode(&bytes[..])
            .map_err(|e| DomainError::Infrastructure(format!("Decode failed: {e}")))?;
        if resp.success {
            Ok(resp.snapshots)
        } else {
            Err(DomainError::Infrastructure(resp.message))
        }
    }

    async fn attach_volume(
        &self,
        host_id: &str,
        vm_id: &str,
        volume_id: &str,
        mount_point: &str,
        read_only: bool,
    ) -> DomainResult<()> {
        self.send_command(
            host_id,
            mikrom_proto::agent::agent_command::Command::AttachVolume(AttachVolumeRequest {
                vm_id: vm_id.to_string(),
                volume_id: volume_id.to_string(),
                mount_point: mount_point.to_string(),
                read_only,
            }),
        )
        .await
    }

    async fn detach_volume(&self, host_id: &str, vm_id: &str, volume_id: &str) -> DomainResult<()> {
        self.send_command(
            host_id,
            mikrom_proto::agent::agent_command::Command::DetachVolume(DetachVolumeRequest {
                vm_id: vm_id.to_string(),
                volume_id: volume_id.to_string(),
            }),
        )
        .await
    }

    async fn start_migration(
        &self,
        host_id: &str,
        vm_id: &str,
        target_host: &str,
        target_uri: &str,
    ) -> DomainResult<()> {
        self.send_command(
            host_id,
            mikrom_proto::agent::agent_command::Command::StartMigration(StartMigrationRequest {
                vm_id: vm_id.to_string(),
                target_host: target_host.to_string(),
                target_uri: target_uri.to_string(),
            }),
        )
        .await
    }

    async fn cancel_migration(&self, host_id: &str, vm_id: &str) -> DomainResult<()> {
        self.send_command(
            host_id,
            mikrom_proto::agent::agent_command::Command::CancelMigration(CancelMigrationRequest {
                vm_id: vm_id.to_string(),
            }),
        )
        .await
    }

    async fn query_migration(&self, host_id: &str, vm_id: &str) -> DomainResult<String> {
        let bytes = self
            .send_command_raw(
                host_id,
                mikrom_proto::agent::agent_command::Command::QueryMigration(
                    QueryMigrationRequest {
                        vm_id: vm_id.to_string(),
                    },
                ),
            )
            .await?;
        let resp = QueryMigrationResponse::decode(&bytes[..])
            .map_err(|e| DomainError::Infrastructure(format!("Decode failed: {e}")))?;
        if resp.success {
            Ok(resp.status)
        } else {
            Err(DomainError::Infrastructure(resp.message))
        }
    }

    async fn set_balloon(
        &self,
        host_id: &str,
        vm_id: &str,
        target_memory_mib: u32,
    ) -> DomainResult<()> {
        self.send_command(
            host_id,
            mikrom_proto::agent::agent_command::Command::SetBalloon(SetBalloonRequest {
                vm_id: vm_id.to_string(),
                target_memory_mib,
            }),
        )
        .await
    }

    async fn query_balloon(&self, host_id: &str, vm_id: &str) -> DomainResult<(u32, u32)> {
        let bytes = self
            .send_command_raw(
                host_id,
                mikrom_proto::agent::agent_command::Command::QueryBalloon(QueryBalloonRequest {
                    vm_id: vm_id.to_string(),
                }),
            )
            .await?;
        let resp = QueryBalloonResponse::decode(&bytes[..])
            .map_err(|e| DomainError::Infrastructure(format!("Decode failed: {e}")))?;
        if resp.success {
            Ok((resp.actual_memory_mib, resp.max_memory_mib))
        } else {
            Err(DomainError::Infrastructure(resp.message))
        }
    }
}
