use crate::firecracker::FirecrackerManager;
use crate::metrics::MetricsCollector;
use crate::types::{AppId, VmId};
use parking_lot::RwLock;
use prost::Message;
use std::collections::HashMap;
use std::sync::Arc;

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
        if let Some(priv_key) = self.config.get_wg_private_key()
            && let Err(e) = self.wg_manager.init(&priv_key, &self.config.host_id)
        {
            tracing::error!("Failed to initialize WireGuard: {e}");
        }

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

                let client = nats_client.as_ref().unwrap();

                // 1. Initialize FirecrackerExporter
                let exporter = crate::metrics::FirecrackerExporter::new(
                    client.clone(),
                    self_clone.metrics_collector.clone(),
                    self_clone.firecracker.clone(),
                );

                // 2. Spawn listeners
                let cmd_handle = self_clone.start_command_listener(client.clone());
                let health_check_handle = self_clone.start_health_check_listener(client.clone());
                let heartbeat_handle = self_clone.start_heartbeat_loop(client.clone());
                let mesh_handle = self_clone
                    .start_mesh_listener(client.clone(), self_clone.config.host_id.clone());
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
    ) -> tokio::task::JoinHandle<()> {
        let wg_manager = self.wg_manager.clone();
        let host_subject = format!("mikrom.scheduler.network.mesh.{}", host_id);
        let config = self.config.clone();

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
            loop {
                let message = tokio::select! {
                    Some(msg) = host_sub.next() => msg,
                    else => break,
                };

                if let Ok(update) =
                    mikrom_proto::scheduler::NetworkMeshUpdate::decode(&message.payload[..])
                    && let Some(priv_key) = config.get_wg_private_key()
                    && let Err(e) =
                        wg_manager.update_peers(&update.peers, &priv_key, &config.host_id)
                {
                    tracing::error!("Failed to update WireGuard peers: {e}");
                }
            }
        })
    }

    fn start_command_listener(&self, client: async_nats::Client) -> tokio::task::JoinHandle<()> {
        let host_id = self.config.host_id.clone();
        let firecracker = self.firecracker.clone();
        let subject = format!("mikrom.agent.{host_id}.cmd");

        tokio::spawn(async move {
            let Ok(mut subscription) = client.subscribe(subject.clone()).await else {
                tracing::error!("Failed to subscribe to commands on {subject}");
                return;
            };

            tracing::info!("Listening for commands on {subject}");
            use futures::StreamExt;
            while let Some(message) = subscription.next().await {
                let fc = firecracker.clone();
                let nats = client.clone();
                tokio::spawn(async move {
                    Self::handle_nats_command(message, fc, nats).await;
                });
            }
        })
    }

    fn start_health_check_listener(
        &self,
        client: async_nats::Client,
    ) -> tokio::task::JoinHandle<()> {
        let host_id = self.config.host_id.clone();
        let firecracker = self.firecracker.clone();
        let subject = format!("mikrom.agent.{host_id}.check_health");
        let http_client = self.http_client.clone();

        tokio::spawn(async move {
            let Ok(mut subscription) = client.subscribe(subject.clone()).await else {
                tracing::error!("Failed to subscribe to health checks on {subject}");
                return;
            };

            tracing::info!("Listening for health checks on {subject}");
            use futures::StreamExt;
            while let Some(message) = subscription.next().await {
                let fc = firecracker.clone();
                let nats = client.clone();
                let http = http_client.clone();
                tokio::spawn(async move {
                    Self::handle_health_check(message, fc, nats, http).await;
                });
            }
        })
    }

    async fn handle_health_check(
        message: async_nats::Message,
        fc: FirecrackerManager,
        nats: async_nats::Client,
        http_client: reqwest::Client,
    ) {
        use mikrom_proto::agent::{CheckHealthRequest, CheckHealthResponse};
        let Ok(req) = CheckHealthRequest::decode(&message.payload[..]) else {
            tracing::error!("Failed to decode CheckHealthRequest");
            return;
        };

        let vm_id = VmId::from(req.vm_id);
        let vm_info = fc.get_vm_info(&vm_id).await;
        let result = if let Some(vm) = vm_info {
            let port = vm.config.port;
            let path = if vm.config.health_check_path.is_empty() {
                "/".to_string()
            } else {
                vm.config.health_check_path.clone()
            };

            let ip = if let Some(ipv6) = &vm.config.ipv6_address {
                if !ipv6.is_empty() {
                    Some(format!("[{}]", ipv6))
                } else {
                    vm.config.ip_address.clone()
                }
            } else {
                vm.config.ip_address.clone()
            };

            if let Some(ip_addr) = ip {
                let url = format!("http://{ip_addr}:{port}{path}");
                tracing::debug!(vm_id = %vm_id, url = %url, "Performing health check...");

                match http_client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => Ok("Healthy".to_string()),
                    Ok(resp) => Err(format!("Unhealthy: HTTP {}", resp.status())),
                    Err(e) => Err(format!("Unhealthy: {e}")),
                }
            } else {
                Err("VM has no IP address".to_string())
            }
        } else {
            Err("VM not found".to_string())
        };

        if let Some(reply) = message.reply {
            let response = match result {
                Ok(msg) => CheckHealthResponse {
                    is_healthy: true,
                    message: msg,
                },
                Err(e) => CheckHealthResponse {
                    is_healthy: false,
                    message: e,
                },
            };
            let mut buf = Vec::new();
            if response.encode(&mut buf).is_ok() {
                let _ = nats.publish(reply, buf.into()).await;
            }
        }
    }

    async fn handle_nats_command(
        message: async_nats::Message,
        fc: FirecrackerManager,
        nats: async_nats::Client,
    ) {
        use mikrom_proto::agent::{AgentCommand, AgentCommandResponse};
        let Ok(agent_cmd) = AgentCommand::decode(&message.payload[..]) else {
            tracing::error!("Failed to decode AgentCommand");
            return;
        };

        let command = agent_cmd.command;
        tracing::info!(command = ?command, "Received command via NATS");

        let result = match command {
            Some(mikrom_proto::agent::agent_command::Command::StartVm(req)) => {
                let mut config = crate::firecracker::config::VmConfig::default();
                if let Some(c) = req.config {
                    config.vcpus = c.vcpus;
                    config.memory_mib = u64::from(c.memory_mib);
                    config.disk_mib = u64::from(c.disk_mib);
                    config.port = c.port;
                    config.env = c.env;
                    config.ip_address = Some(c.ip_address).filter(|s| !s.is_empty());
                    config.gateway = Some(c.gateway).filter(|s| !s.is_empty());
                    config.ipv6_address = Some(c.ipv6_address).filter(|s| !s.is_empty());
                    config.ipv6_gateway = Some(c.ipv6_gateway).filter(|s| !s.is_empty());
                    config.mac_address = Some(c.mac_address).filter(|s| !s.is_empty());
                    config.netmask = Some(c.netmask).filter(|s| !s.is_empty());
                }

                fc.start_vm(
                    VmId::from(req.vm_id),
                    AppId::from(req.app_id),
                    req.image,
                    config,
                )
                .await
                .map(|_| "VM started".to_string())
            },
            Some(mikrom_proto::agent::agent_command::Command::StopVm(req)) => fc
                .stop_vm(&VmId::from(req.vm_id))
                .await
                .map(|_| "VM stopped".to_string()),
            Some(mikrom_proto::agent::agent_command::Command::PauseVm(req)) => fc
                .pause_vm(&VmId::from(req.vm_id))
                .await
                .map(|_| "VM paused".to_string()),
            Some(mikrom_proto::agent::agent_command::Command::ResumeVm(req)) => fc
                .resume_vm(&VmId::from(req.vm_id))
                .await
                .map(|_| "VM resumed".to_string()),
            Some(mikrom_proto::agent::agent_command::Command::DeleteVm(req)) => fc
                .delete_vm(&VmId::from(req.vm_id))
                .await
                .map(|_| "VM resources purged".to_string()),
            Some(mikrom_proto::agent::agent_command::Command::UpdateFirewall(req)) => {
                use mikrom_agent_ebpf_common::{Action, Protocol};

                let rules: Vec<mikrom_agent_ebpf_common::FirewallRule> = req
                    .rules
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
                    .collect();

                fc.update_vm_firewall(&VmId::from(req.vm_id), rules)
                    .await
                    .map(|_| "Firewall rules updated".to_string())
                    .map_err(|e| {
                        crate::firecracker::config::FirecrackerError::ProcessError(e.to_string())
                    })
            },
            None => Err(crate::firecracker::config::FirecrackerError::ProcessError(
                "Empty command".to_string(),
            )),
        };

        if let Some(reply) = message.reply {
            let response = match result {
                Ok(msg) => AgentCommandResponse {
                    success: true,
                    message: msg,
                },
                Err(e) => AgentCommandResponse {
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

    fn start_heartbeat_loop(&self, client: async_nats::Client) -> tokio::task::JoinHandle<()> {
        let host_id = self.config.host_id.clone();
        let hostname = self.config.hostname();
        let ip_address = self.ip_address.clone();
        let bridge_ip = self.config.bridge_ip.clone();
        let wireguard_pubkey = self.config.wireguard_pubkey.clone().unwrap_or_default();
        let wireguard_ip = self.wg_manager.get_host_ipv6(&host_id);
        let metrics_collector = self.metrics_collector.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
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
                                ip_address: vm.ip_address.clone().unwrap_or_default(),
                                tx_bytes: vm.tx_bytes,
                                rx_bytes: vm.rx_bytes,
                            },
                        )
                    })
                    .collect::<HashMap<String, VmMetrics>>();

                let heartbeat = WorkerHeartbeat {
                    host_id: host_id.clone(),
                    hostname: hostname.clone(),
                    ip_address: ip_address.clone(),
                    bridge_ip: bridge_ip.clone(),
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
                };

                let mut payload = Vec::new();
                if heartbeat.encode(&mut payload).is_ok()
                    && let Err(e) = client
                        .publish("mikrom.scheduler.worker.heartbeat", payload.into())
                        .await
                {
                    tracing::error!("Failed to publish heartbeat: {e}");
                    break;
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
    use mikrom_proto::agent::{CheckHealthRequest, CheckHealthResponse};
    use prost::Message;

    #[tokio::test]
    async fn test_handle_health_check_vm_not_found() {
        let nats_url =
            std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
        let client = async_nats::connect(&nats_url).await.unwrap();

        let fc = FirecrackerManager::new().await;
        let reply = "test.reply.health".to_string();
        let mut sub = client.subscribe(reply.clone()).await.unwrap();

        let req = CheckHealthRequest {
            vm_id: "non-existent-vm".to_string(),
        };
        let mut payload = Vec::new();
        req.encode(&mut payload).unwrap();

        let payload_len = payload.len();
        let message = async_nats::Message {
            subject: "test.subject".into(),
            reply: Some(reply.clone().into()),
            payload: payload.into(),
            headers: None,
            status: None,
            description: None,
            length: payload_len,
        };

        let http = reqwest::Client::new();
        AgentServer::handle_health_check(message, fc, client.clone(), http).await;

        use futures::StreamExt;
        let resp_msg = tokio::time::timeout(std::time::Duration::from_secs(2), sub.next())
            .await
            .expect("Timeout waiting for health check response")
            .expect("No message received");

        let resp = CheckHealthResponse::decode(&resp_msg.payload[..]).unwrap();
        assert!(!resp.is_healthy);
        assert_eq!(resp.message, "VM not found");
    }

    #[tokio::test]
    async fn test_handle_health_check_http_logic() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        // 1. Setup mock HTTP server
        let mock_server = MockServer::start().await;
        let mock_port = mock_server.address().port();
        let mock_ip = mock_server.address().ip().to_string();

        // Expect a hit on /custom-health and return 200
        Mock::given(method("GET"))
            .and(path("/custom-health"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        // 2. Setup Agent dependencies
        let nats_url =
            std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
        let nats_client = async_nats::connect(&nats_url).await.unwrap();
        let fc = FirecrackerManager::new().await;
        let reply = "test.reply.http".to_string();
        let mut sub = nats_client.subscribe(reply.clone()).await.unwrap();

        // 3. Register a fake VM in the manager so get_vm_info returns it
        let vm_id = VmId::from("test-vm-http");
        {
            use crate::firecracker::config::{VmConfig, VmInfo, VmStatus};
            let mut vms = fc.vms.write().await;
            vms.insert(
                vm_id.clone(),
                VmInfo {
                    vm_id: vm_id.clone(),
                    app_id: AppId::from("app-1"),
                    image: "img".into(),
                    status: VmStatus::Running,
                    started_at: None,
                    error_message: None,
                    config: VmConfig {
                        ip_address: Some(mock_ip),
                        port: mock_port as u32,
                        health_check_path: "/custom-health".into(),
                        ..Default::default()
                    },
                },
            );
        }

        // 4. Send health check request
        let req = CheckHealthRequest {
            vm_id: vm_id.to_string(),
        };
        let mut payload = Vec::new();
        req.encode(&mut payload).unwrap();
        let payload_len = payload.len();
        let message = async_nats::Message {
            subject: "test.subject".into(),
            reply: Some(reply.clone().into()),
            payload: payload.into(),
            headers: None,
            status: None,
            description: None,
            length: payload_len,
        };

        let http = reqwest::Client::new();
        AgentServer::handle_health_check(message, fc.clone(), nats_client.clone(), http).await;

        // 5. Verify success (200 OK on custom path)
        use futures::StreamExt;
        let resp_msg = tokio::time::timeout(std::time::Duration::from_secs(2), sub.next())
            .await
            .expect("Timeout waiting for health check response")
            .unwrap();
        let resp = CheckHealthResponse::decode(&resp_msg.payload[..]).unwrap();
        assert!(
            resp.is_healthy,
            "Should be healthy for 200 OK: {}",
            resp.message
        );

        // 6. Test 302 Redirect (should be Unhealthy now)
        Mock::given(method("GET"))
            .and(path("/redirect"))
            .respond_with(ResponseTemplate::new(302))
            .mount(&mock_server)
            .await;

        {
            let mut vms = fc.vms.write().await;
            vms.get_mut(&vm_id).unwrap().config.health_check_path = "/redirect".into();
        }

        let mut payload = Vec::new();
        req.encode(&mut payload).unwrap();
        let payload_len = payload.len();
        let message = async_nats::Message {
            subject: "test.subject".into(),
            reply: Some(reply.clone().into()),
            payload: payload.into(),
            headers: None,
            status: None,
            description: None,
            length: payload_len,
        };

        AgentServer::handle_health_check(message, fc, nats_client, reqwest::Client::new()).await;

        let resp_msg = tokio::time::timeout(std::time::Duration::from_secs(2), sub.next())
            .await
            .expect("Timeout waiting for second response")
            .unwrap();
        let resp = CheckHealthResponse::decode(&resp_msg.payload[..]).unwrap();
        assert!(!resp.is_healthy, "Should be unhealthy for 302 Redirect");
        assert!(resp.message.contains("HTTP 302 Found"));
    }
}
