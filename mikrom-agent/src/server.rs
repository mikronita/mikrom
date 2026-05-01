use crate::firecracker::FirecrackerManager;
use crate::metrics::MetricsCollector;
use parking_lot::RwLock;
use prost::Message;
use std::net::SocketAddr;
use std::sync::Arc;

pub struct AgentServer {
    config: crate::config::AgentConfig,
    ip_address: String,
    metrics_collector: MetricsCollector,
    firecracker: FirecrackerManager,
    shutdown_flag: Arc<RwLock<bool>>,
}

impl AgentServer {
    #[must_use]
    pub fn new(config: crate::config::AgentConfig, ip_address: String) -> Self {
        let firecracker = FirecrackerManager::new();
        Self::with_manager(config, ip_address, firecracker)
    }

    #[must_use]
    pub fn with_manager(
        config: crate::config::AgentConfig,
        ip_address: String,
        firecracker: FirecrackerManager,
    ) -> Self {
        Self {
            config: config.clone(),
            ip_address,
            metrics_collector: MetricsCollector::with_firecracker(firecracker.clone()),
            firecracker,
            shutdown_flag: Arc::new(RwLock::new(false)),
        }
    }

    pub async fn serve(&self, addr: SocketAddr) -> anyhow::Result<()> {
        // Initialize global networking (bridge, forwarding, NAT)
        if let Err(e) = self.firecracker.init_network().await {
            tracing::error!("Failed to initialize host networking: {e}");
        }

        // Cleanup any stale resources from previous runs
        self.firecracker.cleanup_all_stale_resources().await;

        // Start background tasks (GC)
        self.firecracker.start_background_tasks();

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

                // Spawn command listener and heartbeat tasks
                let cmd_handle = self_clone.start_command_listener(client.clone());
                let heartbeat_handle = self_clone.start_heartbeat_loop(client.clone(), addr.port());

                tokio::select! {
                    _ = cmd_handle => tracing::warn!("Command listener exited"),
                    _ = heartbeat_handle => {
                        tracing::warn!("Heartbeat loop exited, forcing NATS reconnect");
                        nats_client = None;
                    }
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
                    config.mac_address = Some(c.mac_address).filter(|s| !s.is_empty());
                    config.netmask = Some(c.netmask).filter(|s| !s.is_empty());
                }
                fc.start_vm(req.vm_id, req.app_id, req.image, config)
                    .await
                    .map(|_| "VM started".to_string())
            },
            Some(mikrom_proto::agent::agent_command::Command::StopVm(req)) => fc
                .stop_vm(&req.vm_id)
                .await
                .map(|_| "VM stopped".to_string()),
            Some(mikrom_proto::agent::agent_command::Command::PauseVm(req)) => fc
                .pause_vm(&req.vm_id)
                .await
                .map(|_| "VM paused".to_string()),
            Some(mikrom_proto::agent::agent_command::Command::ResumeVm(req)) => fc
                .resume_vm(&req.vm_id)
                .await
                .map(|_| "VM resumed".to_string()),
            Some(mikrom_proto::agent::agent_command::Command::DeleteVm(req)) => fc
                .delete_vm(&req.vm_id)
                .await
                .map(|_| "VM resources purged".to_string()),
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

    fn start_heartbeat_loop(
        &self,
        client: async_nats::Client,
        agent_port: u16,
    ) -> tokio::task::JoinHandle<()> {
        let host_id = self.config.host_id.clone();
        let hostname = self.config.hostname();
        let ip_address = self.ip_address.clone();
        let bridge_ip = self.config.bridge_ip.clone();
        let metrics_collector = self.metrics_collector.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
            loop {
                interval.tick().await;
                let metrics = metrics_collector.collect().await;

                use mikrom_proto::scheduler::{
                    ReportMetricsRequest, VmMetrics, VmStatus as ProtoVmStatus, WorkerHeartbeat,
                };

                let vms: std::collections::HashMap<String, VmMetrics> = metrics
                    .vms
                    .iter()
                    .map(|(id, vm)| {
                        (
                            id.clone(),
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
                            },
                        )
                    })
                    .collect();

                let heartbeat = WorkerHeartbeat {
                    host_id: host_id.clone(),
                    hostname: hostname.clone(),
                    ip_address: ip_address.clone(),
                    agent_port: u32::from(agent_port),
                    bridge_ip: bridge_ip.clone(),
                    metrics: Some(ReportMetricsRequest {
                        host_id: host_id.clone(),
                        cpu_usage: metrics.cpu_usage,
                        ram_used_bytes: metrics.ram_used_bytes,
                        ram_total_bytes: metrics.ram_total_bytes,
                        disk_used_bytes: metrics.disk_used_bytes,
                        disk_total_bytes: metrics.disk_total_bytes,
                        apps_count: metrics.apps_count,
                        timestamp: chrono::Utc::now().timestamp(),
                        load_avg_1: metrics.load_avg_1,
                        load_avg_5: metrics.load_avg_5,
                        load_avg_15: metrics.load_avg_15,
                        vms,
                    }),
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
        }
    }
}
