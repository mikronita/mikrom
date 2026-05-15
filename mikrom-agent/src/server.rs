use crate::ceph::StorageProvider;
use crate::firecracker::FirecrackerManager;
use crate::metrics::MetricsCollector;
use parking_lot::RwLock;
use prost::Message;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

pub struct AgentServer {
    config: crate::config::AgentConfig,
    ip_address: String,
    metrics_collector: MetricsCollector,
    firecracker: FirecrackerManager,
    shutdown_flag: Arc<RwLock<bool>>,
    http_client: reqwest::Client,
    wg_manager: Arc<crate::wireguard::WireGuardManager>,
}

impl AgentServer {
    pub async fn new(config: crate::config::AgentConfig, ip_address: String) -> Self {
        let firecracker = FirecrackerManager::new().await;
        Self::with_manager(config, ip_address, firecracker)
    }

    #[must_use]
    pub fn with_manager(
        config: crate::config::AgentConfig,
        ip_address: String,
        firecracker: FirecrackerManager,
    ) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap_or_default();

        Self {
            config: config.clone(),
            ip_address,
            metrics_collector: MetricsCollector::with_firecracker(firecracker.clone()),
            firecracker,
            shutdown_flag: Arc::new(RwLock::new(false)),
            http_client,
            wg_manager: Arc::new(crate::wireguard::WireGuardManager::new("wg0")),
        }
    }

    pub async fn serve(&self) -> anyhow::Result<()> {
        // Initialize global networking (bridge, forwarding, NAT)
        if let Err(e) = self.firecracker.init_network().await {
            tracing::error!("Failed to initialize host networking: {e}");
        }

        // Cleanup any stale resources from previous runs
        self.firecracker.cleanup_all_stale_resources().await;

        // Start background tasks (GC)
        self.firecracker.start_background_tasks();

        // 3. Initialize WireGuard
        let priv_key = match self.config.get_wg_private_key() {
            Some(key) => key,
            None => {
                info!("WireGuard private key not provided, attempting to load or generate...");
                self.wg_manager
                    .load_or_generate_key(&self.firecracker.fc_config.data_dir)
                    .await?
            },
        };

        if let Err(e) = self.wg_manager.init(&priv_key, &self.config.host_id).await {
            tracing::error!("Failed to initialize WireGuard: {e}");
        }

        let pub_key = self.wg_manager.get_public_key(&priv_key)?;

        let nats_url = self.config.nats_url.clone();
        let firecracker = self.firecracker.clone();
        let self_clone = self.clone();

        tokio::spawn(async move {
            let mut nats_client = None;

            loop {
                if nats_client.is_none() {
                    tracing::info!("Connecting to NATS at {nats_url}");
                    match async_nats::connect(&nats_url).await {
                        Ok(client) => {
                            tracing::info!("Connected to NATS");
                            nats_client = Some(client.clone());
                            firecracker.set_nats_client(client).await;
                        },
                        Err(e) => {
                            tracing::error!("Failed to connect to NATS: {e}. Retrying in 5s...");
                            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                            continue;
                        },
                    }
                }

                let Some(client) = nats_client.as_ref() else {
                    continue;
                };

                // 1. Initialize FirecrackerExporter
                let exporter = crate::metrics::FirecrackerExporter::new(
                    client.clone(),
                    self_clone.metrics_collector.clone(),
                    self_clone.firecracker.clone(),
                );

                // 2. Spawn listeners
                let cmd_handle = self_clone.start_command_listener(client.clone());
                let health_check_handle = self_clone.start_health_check_listener(client.clone());
                let heartbeat_handle =
                    self_clone.start_heartbeat_loop(client.clone(), pub_key.clone());
                let mesh_handle = self_clone.start_mesh_listener(
                    client.clone(),
                    self_clone.config.host_id.clone(),
                    priv_key.clone(),
                );
                let exporter_handle = tokio::spawn(async move {
                    exporter.start_export_loop().await;
                });

                tokio::select! {
                    _ = cmd_handle => tracing::warn!("Command listener exited"),
                    _ = health_check_handle => tracing::warn!("Health check listener exited"),
                    _ = heartbeat_handle => {
                        tracing::warn!("Heartbeat loop exited, forcing NATS reconnect");
                        nats_client = None;
                    }
                    _ = mesh_handle => tracing::warn!("Mesh listener exited"),
                    _ = exporter_handle => tracing::warn!("Exporter loop exited"),
                }
            }
        });

        // Wait for shutdown flag
        while !*self.shutdown_flag.read() {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }

        tracing::info!("Agent shutdown requested");
        Ok(())
    }

    fn start_mesh_listener(
        &self,
        client: async_nats::Client,
        host_id: String,
        priv_key: String,
    ) -> tokio::task::JoinHandle<()> {
        let wg_manager = self.wg_manager.clone();
        let host_subject = format!("mikrom.scheduler.network.mesh.{}", host_id);

        tokio::spawn(async move {
            let mut host_sub = match client.subscribe(host_subject.clone()).await {
                Ok(sub) => sub,
                Err(e) => {
                    tracing::error!("Failed to subscribe to host mesh updates: {e}");
                    return;
                },
            };

            tracing::info!("Listening for mesh updates on {}", host_subject);
            use futures::StreamExt;
            while let Some(msg) = host_sub.next().await {
                if let Ok(update) =
                    mikrom_proto::scheduler::NetworkMeshUpdate::decode(&msg.payload[..])
                {
                    info!("Received mesh update with {} peers", update.peers.len());
                    if let Err(e) = wg_manager
                        .update_peers(&update.peers, &priv_key, &host_id)
                        .await
                    {
                        tracing::error!("Failed to update WireGuard peers: {e}");
                    }
                }
            }
        })
    }

    fn start_command_listener(&self, client: async_nats::Client) -> tokio::task::JoinHandle<()> {
        let fc = self.firecracker.clone();
        let host_id = self.config.host_id.clone();
        // Fixed subject to match scheduler: mikrom.agent.{host_id}.cmd
        let subject = format!("mikrom.agent.{}.cmd", host_id);
        let nats = client.clone();

        tokio::spawn(async move {
            let mut cmd_sub = match client.subscribe(subject.clone()).await {
                Ok(sub) => sub,
                Err(e) => {
                    tracing::error!("Failed to subscribe to agent commands: {e}");
                    return;
                },
            };

            tracing::info!("Listening for agent commands on {}", subject);
            use futures::StreamExt;
            while let Some(msg) = cmd_sub.next().await {
                Self::handle_nats_command(msg, &fc, &nats).await;
            }
        })
    }

    fn start_health_check_listener(
        &self,
        client: async_nats::Client,
    ) -> tokio::task::JoinHandle<()> {
        let fc = self.firecracker.clone();
        let host_id = self.config.host_id.clone();
        // Fixed subject to match scheduler: mikrom.agent.{host_id}.check_health
        let subject = format!("mikrom.agent.{}.check_health", host_id);
        let nats = client.clone();
        let http_client = self.http_client.clone();

        tokio::spawn(async move {
            let mut health_sub = match client.subscribe(subject.clone()).await {
                Ok(sub) => sub,
                Err(e) => {
                    tracing::error!("Failed to subscribe to health checks: {e}");
                    return;
                },
            };

            tracing::info!("Listening for health checks on {}", subject);
            use futures::StreamExt;
            while let Some(msg) = health_sub.next().await {
                Self::handle_health_check(msg, &fc, &nats, &http_client).await;
            }
        })
    }

    async fn handle_nats_command(
        message: async_nats::Message,
        fc: &FirecrackerManager,
        nats: &async_nats::Client,
    ) {
        use mikrom_proto::agent::AgentCommand;
        let Ok(command) = AgentCommand::decode(&message.payload[..]) else {
            tracing::error!("Failed to decode AgentCommand");
            return;
        };

        let result = Self::dispatch_agent_command(command.command, fc).await;
        Self::reply_agent_command(message, nats, result).await;
    }

    async fn dispatch_agent_command(
        command: Option<mikrom_proto::agent::agent_command::Command>,
        fc: &FirecrackerManager,
    ) -> Result<String, crate::firecracker::config::FirecrackerError> {
        match command {
            Some(mikrom_proto::agent::agent_command::Command::StartVm(req)) => {
                let config = Self::proto_vm_config(req.config);
                let vm_id = Self::parse_vm_id(&req.vm_id)?;
                let app_id = Self::parse_app_id(&req.app_id)?;

                fc.start_vm(vm_id, app_id, req.image, config)
                    .await
                    .map(|_| "VM started".to_string())
            },
            Some(mikrom_proto::agent::agent_command::Command::StopVm(req)) => {
                let vm_id = Self::parse_vm_id(&req.vm_id)?;
                fc.stop_vm(&vm_id).await.map(|_| "VM stopped".to_string())
            },
            Some(mikrom_proto::agent::agent_command::Command::PauseVm(req)) => {
                let vm_id = Self::parse_vm_id(&req.vm_id)?;
                fc.pause_vm(&vm_id).await.map(|_| "VM paused".to_string())
            },
            Some(mikrom_proto::agent::agent_command::Command::ResumeVm(req)) => {
                let vm_id = Self::parse_vm_id(&req.vm_id)?;
                fc.resume_vm(&vm_id).await.map(|_| "VM resumed".to_string())
            },
            Some(mikrom_proto::agent::agent_command::Command::DeleteVm(req)) => {
                let vm_id = Self::parse_vm_id(&req.vm_id)?;
                fc.delete_vm(&vm_id)
                    .await
                    .map(|_| "VM resources purged".to_string())
            },
            Some(mikrom_proto::agent::agent_command::Command::UpdateFirewall(req)) => {
                let vm_id = Self::parse_vm_id(&req.vm_id)?;
                let rules = Self::map_firewall_rules(req.rules);

                fc.update_vm_firewall(&vm_id, rules)
                    .await
                    .map(|_| "Firewall rules updated".to_string())
                    .map_err(|e| {
                        crate::firecracker::config::FirecrackerError::ProcessError(e.to_string())
                    })
            },
            Some(mikrom_proto::agent::agent_command::Command::CreateSnapshot(req)) => {
                let storage = crate::ceph::CephRbd;
                storage
                    .create_snapshot(&req.pool_name, &req.volume_id, &req.snapshot_name)
                    .map(|_| "Snapshot created".to_string())
                    .map_err(|e| {
                        crate::firecracker::config::FirecrackerError::ProcessError(e.to_string())
                    })
            },
            Some(mikrom_proto::agent::agent_command::Command::DeleteVolume(req)) => {
                let storage = crate::ceph::CephRbd;
                storage
                    .delete_volume(&req.pool_name, &req.volume_id)
                    .map(|_| "Volume deleted".to_string())
                    .map_err(|e| {
                        crate::firecracker::config::FirecrackerError::ProcessError(e.to_string())
                    })
            },
            Some(mikrom_proto::agent::agent_command::Command::DeleteSnapshot(req)) => {
                let storage = crate::ceph::CephRbd;
                storage
                    .delete_snapshot(&req.pool_name, &req.volume_id, &req.snapshot_name)
                    .map(|_| "Snapshot deleted".to_string())
                    .map_err(|e| {
                        crate::firecracker::config::FirecrackerError::ProcessError(e.to_string())
                    })
            },
            Some(mikrom_proto::agent::agent_command::Command::CreateVolume(req)) => {
                let storage = crate::ceph::CephRbd;
                storage
                    .create_volume(&req.pool_name, &req.volume_id, req.size_mib as i32)
                    .map(|_| "Volume created".to_string())
                    .map_err(|e| {
                        crate::firecracker::config::FirecrackerError::ProcessError(e.to_string())
                    })
            },
            Some(mikrom_proto::agent::agent_command::Command::RestoreSnapshot(req)) => {
                let storage = crate::ceph::CephRbd;
                storage
                    .restore_snapshot(&req.pool_name, &req.volume_id, &req.snapshot_name)
                    .map(|_| "Snapshot restored".to_string())
                    .map_err(|e| {
                        crate::firecracker::config::FirecrackerError::ProcessError(e.to_string())
                    })
            },
            Some(mikrom_proto::agent::agent_command::Command::CloneVolume(req)) => {
                let storage = crate::ceph::CephRbd;
                storage
                    .clone_volume(
                        &req.pool_name,
                        &req.source_volume_id,
                        &req.snapshot_name,
                        &req.target_volume_id,
                    )
                    .map(|_| "Volume cloned".to_string())
                    .map_err(|e| {
                        crate::firecracker::config::FirecrackerError::ProcessError(e.to_string())
                    })
            },
            None => Err(crate::firecracker::config::FirecrackerError::ProcessError(
                "Empty command".to_string(),
            )),
        }
    }

    fn proto_vm_config(
        config: Option<mikrom_proto::agent::VmConfig>,
    ) -> crate::firecracker::config::VmConfig {
        let mut vm_config = crate::firecracker::config::VmConfig::default();
        if let Some(c) = config {
            vm_config.vcpus = c.vcpus;
            vm_config.memory_mib = u64::from(c.memory_mib);
            vm_config.disk_mib = u64::from(c.disk_mib);
            vm_config.port = c.port;
            vm_config.env = c.env;
            vm_config.ipv6_address = Some(c.ipv6_address).filter(|s| !s.is_empty());
            vm_config.ipv6_gateway = Some(c.ipv6_gateway).filter(|s| !s.is_empty());
            vm_config.volumes = c
                .volumes
                .into_iter()
                .map(|v| crate::firecracker::config::Volume {
                    volume_id: v.volume_id,
                    size_mib: v.size_mib,
                    read_only: v.read_only,
                    pool_name: v.pool_name,
                })
                .collect();
        }
        vm_config
    }

    fn map_firewall_rules(
        rules: Vec<mikrom_proto::agent::FirewallRule>,
    ) -> Vec<mikrom_agent_ebpf_common::FirewallRule> {
        use mikrom_agent_ebpf_common::{Action, Protocol};

        rules
            .into_iter()
            .map(|r| mikrom_agent_ebpf_common::FirewallRule {
                protocol: match r.protocol.to_lowercase().as_str() {
                    "tcp" => Protocol::Tcp,
                    "udp" => Protocol::Udp,
                    _ => Protocol::Any,
                },
                port_start: r.port_start as u16,
                port_end: r.port_end as u16,
                action: if r.action.to_lowercase() == "allow" {
                    Action::Allow
                } else {
                    Action::Deny
                },
                remote_ip: [0u8; 16],
                remote_prefix: 0,
            })
            .collect()
    }

    fn parse_vm_id(
        vm_id: &str,
    ) -> Result<mikrom_proto::id::VmId, crate::firecracker::config::FirecrackerError> {
        vm_id.parse::<mikrom_proto::id::VmId>().map_err(|e| {
            crate::firecracker::config::FirecrackerError::ProcessError(format!(
                "Invalid vm_id '{vm_id}': {e}"
            ))
        })
    }

    fn parse_app_id(
        app_id: &str,
    ) -> Result<mikrom_proto::id::AppId, crate::firecracker::config::FirecrackerError> {
        app_id.parse::<mikrom_proto::id::AppId>().map_err(|e| {
            crate::firecracker::config::FirecrackerError::ProcessError(format!(
                "Invalid app_id '{app_id}': {e}"
            ))
        })
    }

    async fn reply_agent_command(
        message: async_nats::Message,
        nats: &async_nats::Client,
        result: Result<String, crate::firecracker::config::FirecrackerError>,
    ) {
        if let Some(reply) = message.reply {
            let response = match result {
                Ok(msg) => mikrom_proto::agent::AgentCommandResponse {
                    success: true,
                    message: msg,
                },
                Err(e) => mikrom_proto::agent::AgentCommandResponse {
                    success: false,
                    message: e.to_string(),
                },
            };
            let mut buf = Vec::new();
            if response.encode(&mut buf).is_ok() {
                let _ = nats.publish(reply, buf.into()).await;
            }
        }
    }

    async fn handle_health_check(
        message: async_nats::Message,
        fc: &FirecrackerManager,
        nats: &async_nats::Client,
        http_client: &reqwest::Client,
    ) {
        use mikrom_proto::agent::CheckHealthRequest;
        let Ok(req) = CheckHealthRequest::decode(&message.payload[..]) else {
            tracing::error!("Failed to decode CheckHealthRequest");
            return;
        };

        let vm_id = req.vm_id.parse().unwrap_or_default();
        let vm_info = fc.get_vm_info(&vm_id).await;
        let result = if let Some(vm) = vm_info {
            let port = vm.config.port;
            let path = if vm.config.health_check_path.is_empty() {
                "/".to_string()
            } else {
                vm.config.health_check_path.clone()
            };
            let ip = vm.config.ipv6_address.clone();

            if let Some(ip_addr) = ip {
                let started_at_ms = fc.get_vm_started_at_ms(&vm_id).await.unwrap_or_default();
                let now_ms = chrono::Utc::now().timestamp_millis() as u64;
                if started_at_ms > 0 && fc.is_app_started(&vm_id).await {
                    let boot_grace_ms = Duration::from_millis(250).as_millis() as u64;
                    if now_ms.saturating_sub(started_at_ms) < boot_grace_ms {
                        tracing::info!(
                            vm_id = %vm_id,
                            ready_since_ms = started_at_ms,
                            boot_grace_ms = boot_grace_ms,
                            "Waiting briefly after application marker before health checking"
                        );
                        return Self::reply_health_check(
                            message,
                            nats,
                            Err("VM application is still booting".to_string()),
                        )
                        .await;
                    }
                }

                let url = if ip_addr.contains(':') {
                    format!("http://[{ip_addr}]:{port}{path}")
                } else {
                    format!("http://{ip_addr}:{port}{path}")
                };
                tracing::info!(
                    vm_id = %vm_id,
                    ip = %ip_addr,
                    port = port,
                    path = %path,
                    url = %url,
                    "Performing health check..."
                );

                match tokio::time::timeout(Duration::from_secs(2), http_client.get(&url).send())
                    .await
                {
                    Ok(Ok(resp)) if resp.status().is_success() => Ok("Healthy".to_string()),
                    Ok(Ok(resp)) => {
                        let status = resp.status();
                        tracing::warn!(
                            vm_id = %vm_id,
                            url = %url,
                            status = %status,
                            "Health check returned non-success status"
                        );
                        Err(format!("Unhealthy: HTTP {}", status))
                    },
                    Ok(Err(e)) => {
                        tracing::warn!(
                            vm_id = %vm_id,
                            url = %url,
                            error = %e,
                            "Health check request failed"
                        );
                        Err(format!("Unhealthy: {e}"))
                    },
                    Err(_) => {
                        tracing::warn!(
                            vm_id = %vm_id,
                            url = %url,
                            "Health check request timed out"
                        );
                        Err("Unhealthy: request timed out".to_string())
                    },
                }
            } else {
                Err("VM has no IPv6 address assigned (6PN required)".to_string())
            }
        } else {
            Err("VM not found".to_string())
        };

        Self::reply_health_check(message, nats, result).await;
    }

    async fn reply_health_check(
        message: async_nats::Message,
        nats: &async_nats::Client,
        result: Result<String, String>,
    ) {
        use mikrom_proto::agent::CheckHealthResponse;

        if let Some(reply) = message.reply {
            let response = match result {
                Ok(msg) => CheckHealthResponse {
                    is_healthy: true,
                    message: msg,
                },
                Err(msg) => CheckHealthResponse {
                    is_healthy: false,
                    message: msg,
                },
            };
            let mut buf = Vec::new();
            if response.encode(&mut buf).is_ok() {
                let _ = nats.publish(reply, buf.into()).await;
            }
        }
    }

    fn start_heartbeat_loop(
        &self,
        client: async_nats::Client,
        pub_key: String,
    ) -> tokio::task::JoinHandle<()> {
        let host_id = self.config.host_id.clone();
        let hostname = self.config.hostname();
        let wireguard_pubkey = pub_key;
        let wireguard_ip = self.wg_manager.get_host_ipv6(&host_id);
        let metrics_collector = self.metrics_collector.clone();
        let advertise_address = self.ip_address.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
            loop {
                interval.tick().await;
                let metrics = metrics_collector.collect().await;

                use mikrom_proto::scheduler::{
                    ReportMetricsRequest, VmMetrics, VmStatus as ProtoVmStatus, WorkerHeartbeat,
                };

                let proto_vms = metrics
                    .vms
                    .iter()
                    .map(|(id, vm)| {
                        (
                            id.to_string(),
                            VmMetrics {
                                cpu_usage: vm.cpu_usage,
                                ram_used_bytes: vm.ram_used_bytes,
                                status: match vm.status {
                                    crate::firecracker::VmStatus::Starting => {
                                        ProtoVmStatus::Starting
                                    },
                                    crate::firecracker::VmStatus::Running => ProtoVmStatus::Running,
                                    crate::firecracker::VmStatus::Paused => ProtoVmStatus::Paused,
                                    crate::firecracker::VmStatus::Stopping => {
                                        ProtoVmStatus::Stopping
                                    },
                                    crate::firecracker::VmStatus::Stopped => ProtoVmStatus::Stopped,
                                    crate::firecracker::VmStatus::Failed => ProtoVmStatus::Failed,
                                } as i32,
                                error_message: vm.error_message.clone().unwrap_or_default(),
                                tx_bytes: vm.tx_bytes,
                                rx_bytes: vm.rx_bytes,
                            },
                        )
                    })
                    .collect::<HashMap<String, VmMetrics>>();

                let heartbeat = WorkerHeartbeat {
                    host_id: host_id.clone(),
                    hostname: hostname.clone(),
                    metrics: Some(ReportMetricsRequest {
                        host_id: host_id.clone(),
                        cpu_usage: metrics.cpu_usage,
                        ram_used_bytes: metrics.ram_used_bytes,
                        ram_total_bytes: metrics.ram_total_bytes,
                        disk_used_bytes: metrics.disk_used_bytes,
                        disk_total_bytes: metrics.disk_total_bytes,
                        apps_count: metrics.apps_count,
                        timestamp: metrics.timestamp,
                        load_avg_1: metrics.load_avg_1,
                        load_avg_5: metrics.load_avg_5,
                        load_avg_15: metrics.load_avg_15,
                        vms: proto_vms,
                    }),
                    wireguard_pubkey: wireguard_pubkey.clone(),
                    wireguard_ip: wireguard_ip.clone(),
                    wireguard_port: 51820,
                    advertise_address: advertise_address.clone(),
                };

                let mut buf = Vec::new();
                if heartbeat.encode(&mut buf).is_ok() {
                    let _ = client
                        .publish("mikrom.scheduler.worker.heartbeat", buf.into())
                        .await;
                }
            }
        })
    }
}

impl Clone for AgentServer {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            ip_address: self.ip_address.clone(),
            metrics_collector: self.metrics_collector.clone(),
            firecracker: self.firecracker.clone(),
            shutdown_flag: self.shutdown_flag.clone(),
            http_client: self.http_client.clone(),
            wg_manager: self.wg_manager.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::firecracker::FirecrackerManager;
    use async_nats::Message as NatsMessage;
    use futures::StreamExt;
    use mikrom_proto::agent::{CheckHealthRequest, CheckHealthResponse};
    use mikrom_proto::id::{AppId, VmId};
    use prost::Message;

    #[tokio::test]
    async fn test_handle_health_check_vm_not_found() {
        let nats_url =
            std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
        let nats_client = async_nats::connect(nats_url).await.unwrap();
        let fc = FirecrackerManager::new().await;
        let reply = "test.reply.notfound".to_string();
        let mut sub = nats_client.subscribe(reply.clone()).await.unwrap();

        let req = CheckHealthRequest {
            vm_id: "ghost-vm".to_string(),
        };
        let mut payload = Vec::new();
        req.encode(&mut payload).unwrap();
        let payload_len = payload.len();

        let message = NatsMessage {
            subject: "test.subject".into(),
            reply: Some(reply.into()),
            payload: payload.into(),
            headers: None,
            status: None,
            description: None,
            length: payload_len,
        };

        AgentServer::handle_health_check(message, &fc, &nats_client, &reqwest::Client::new()).await;

        let resp_msg = tokio::time::timeout(std::time::Duration::from_secs(2), sub.next())
            .await
            .unwrap()
            .unwrap();
        let resp = CheckHealthResponse::decode(&resp_msg.payload[..]).unwrap();
        assert!(!resp.is_healthy);
        assert!(resp.message.contains("VM not found"));
    }

    #[tokio::test]
    async fn test_handle_health_check_http_logic() {
        // 1. Setup a mock HTTP server
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&server)
            .await;

        // 2. Setup NATS
        let nats_url =
            std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
        let nats_client = async_nats::connect(nats_url).await.unwrap();
        let fc = FirecrackerManager::new().await;
        let reply = "test.reply.http".to_string();
        let mut sub = nats_client.subscribe(reply.clone()).await.unwrap();

        // 3. Register a fake VM in the manager so get_vm_info returns it
        let vm_id = VmId::new();
        {
            use crate::firecracker::config::{VmConfig, VmInfo, VmStatus};
            let mut vms = fc.vms.write().await;
            vms.insert(
                vm_id,
                VmInfo {
                    vm_id,
                    app_id: AppId::new(),
                    image: "nginx".to_string(),
                    status: VmStatus::Running,
                    config: VmConfig {
                        port: server.address().port() as u32,
                        health_check_path: "/".to_string(),
                        ipv6_address: Some(server.address().ip().to_string()),
                        ..Default::default()
                    },
                    started_at: None,
                    error_message: None,
                },
            );
            let started_at_ms =
                (chrono::Utc::now().timestamp_millis() as u64).saturating_sub(1_000);
            fc.seed_started_process_for_test(vm_id, started_at_ms).await;
        }

        // 4. Send health check request
        let req = CheckHealthRequest {
            vm_id: vm_id.to_string(),
        };
        let mut payload = Vec::new();
        req.encode(&mut payload).unwrap();
        let payload_len = payload.len();
        let message = NatsMessage {
            subject: "test.subject".into(),
            reply: Some(reply.clone().into()),
            payload: payload.into(),
            headers: None,
            status: None,
            description: None,
            length: payload_len,
        };

        AgentServer::handle_health_check(message, &fc, &nats_client, &reqwest::Client::new()).await;

        let resp_msg = tokio::time::timeout(std::time::Duration::from_secs(2), sub.next())
            .await
            .unwrap()
            .unwrap();
        let resp = CheckHealthResponse::decode(&resp_msg.payload[..]).unwrap();
        assert!(
            resp.is_healthy,
            "Should be healthy for 200 OK: {}",
            resp.message
        );

        // 5. Update it to a redirecting path
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/redirect"))
            .respond_with(wiremock::ResponseTemplate::new(302))
            .mount(&server)
            .await;

        {
            let mut vms = fc.vms.write().await;
            vms.get_mut(&vm_id).unwrap().config.health_check_path = "/redirect".into();
        }

        let mut payload2 = Vec::new();
        req.encode(&mut payload2).unwrap();
        let payload_len2 = payload2.len();
        let message2 = NatsMessage {
            subject: "test.subject".into(),
            reply: Some(reply.into()),
            payload: payload2.into(),
            headers: None,
            status: None,
            description: None,
            length: payload_len2,
        };

        AgentServer::handle_health_check(message2, &fc, &nats_client, &reqwest::Client::new())
            .await;
        let resp_msg = tokio::time::timeout(std::time::Duration::from_secs(2), sub.next())
            .await
            .unwrap()
            .unwrap();
        let resp = CheckHealthResponse::decode(&resp_msg.payload[..]).unwrap();
        assert!(!resp.is_healthy, "Should be unhealthy for 302 Redirect");
        assert!(resp.message.contains("HTTP 302 Found"));
    }
}
