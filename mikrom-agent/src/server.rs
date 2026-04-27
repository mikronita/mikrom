use crate::firecracker::FirecrackerManager;
use crate::metrics::MetricsCollector;
use mikrom_proto::tls::ServiceCerts;
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
        let host_id = self.config.host_id.clone();
        let hostname = self.config.hostname();
        let ip_address = self.ip_address.clone();
        let metrics_collector = self.metrics_collector.clone();
        let agent_port = addr.port();
        let use_tls = self.config.use_tls;

        // TLS certificates are no longer needed for gRPC but keeping the logic if needed for other things
        let _certs: Option<ServiceCerts> = if use_tls {
            Some(ServiceCerts::load(&self.config.certs_dir)?)
        } else {
            None
        };

        // Initialize global networking (bridge, forwarding, NAT)
        if let Err(e) = self.firecracker.init_network().await {
            tracing::error!(
                "Failed to initialize host networking: {}. VMs might not have internet access.",
                e
            );
        }

        // Cleanup any stale resources from previous runs
        self.firecracker.cleanup_all_stale_resources().await;

        // Start background tasks (GC)
        self.firecracker.start_background_tasks();

        let nats_url = self.config.nats_url.clone();
        let bridge_ip = self.config.bridge_ip.clone();
        let firecracker = self.firecracker.clone();

        tokio::spawn(async move {
            let mut nats_client = None;

            // ── Main Loop (NATS Connection + Heartbeat) ──────────────────────
            loop {
                if nats_client.is_none() {
                    tracing::info!("Connecting to NATS at {}", nats_url);
                    match async_nats::connect(&nats_url).await {
                        Ok(client) => {
                            tracing::info!("Connected to NATS");
                            nats_client = Some(client.clone());
                            firecracker.set_nats_client(client).await;
                        },
                        Err(e) => {
                            tracing::error!("Failed to connect to NATS: {}. Retrying in 5s...", e);
                            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                            continue;
                        },
                    }
                }

                let client = nats_client.as_ref().unwrap();

                // ── Command Listener ─────────────────────────────────────────
                let host_id_cmd = host_id.clone();
                let client_cmd = client.clone();
                let firecracker_cmd = firecracker.clone();

                tokio::spawn(async move {
                    let subject = format!("mikrom.agent.{}.cmd", host_id_cmd);
                    tracing::info!("Listening for commands on {}", subject);

                    let mut subscription = match client_cmd.subscribe(subject).await {
                        Ok(sub) => sub,
                        Err(e) => {
                            tracing::error!("Failed to subscribe to commands: {}", e);
                            return;
                        },
                    };

                    use futures::StreamExt;
                    use mikrom_proto::agent::{AgentCommand, AgentCommandResponse};
                    use prost::Message;

                    while let Some(message) = subscription.next().await {
                        let firecracker = firecracker_cmd.clone();
                        let client = client_cmd.clone();

                        tokio::spawn(async move {
                            if let Ok(agent_cmd) = AgentCommand::decode(&message.payload[..]) {
                                let command = agent_cmd.command;
                                tracing::info!(command = ?command, "Received command via NATS (Protobuf)");

                                let result = match command {
                                    Some(mikrom_proto::agent::agent_command::Command::StartVm(
                                        req,
                                    )) => {
                                        let mut config =
                                            crate::firecracker::config::VmConfig::default();
                                        if let Some(c) = req.config {
                                            config.vcpus = c.vcpus;
                                            config.memory_mib = u64::from(c.memory_mib);
                                            config.disk_mib = u64::from(c.disk_mib);
                                            config.port = c.port;
                                            config.env = c.env;
                                            config.ip_address =
                                                Some(c.ip_address).filter(|s| !s.is_empty());
                                            config.gateway =
                                                Some(c.gateway).filter(|s| !s.is_empty());
                                            config.mac_address =
                                                Some(c.mac_address).filter(|s| !s.is_empty());
                                            config.netmask =
                                                Some(c.netmask).filter(|s| !s.is_empty());
                                        }

                                        firecracker
                                            .start_vm(req.vm_id, req.app_id, req.image, config)
                                            .await
                                            .map(|_| "VM started".to_string())
                                            .map_err(|e| e.to_string())
                                    },
                                    Some(mikrom_proto::agent::agent_command::Command::StopVm(
                                        req,
                                    )) => firecracker
                                        .stop_vm(&req.vm_id)
                                        .await
                                        .map(|_| "VM stopped".to_string())
                                        .map_err(|e| e.to_string()),
                                    Some(mikrom_proto::agent::agent_command::Command::PauseVm(
                                        req,
                                    )) => firecracker
                                        .pause_vm(&req.vm_id)
                                        .await
                                        .map(|_| "VM paused".to_string())
                                        .map_err(|e| e.to_string()),
                                    Some(
                                        mikrom_proto::agent::agent_command::Command::ResumeVm(req),
                                    ) => firecracker
                                        .resume_vm(&req.vm_id)
                                        .await
                                        .map(|_| "VM resumed".to_string())
                                        .map_err(|e| e.to_string()),
                                    Some(
                                        mikrom_proto::agent::agent_command::Command::DeleteVm(req),
                                    ) => firecracker
                                        .delete_vm(&req.vm_id)
                                        .await
                                        .map(|_| "VM resources purged".to_string())
                                        .map_err(|e| e.to_string()),
                                    None => Err("Empty command received".to_string()),
                                };

                                if let Some(reply) = message.reply {
                                    let response = match result {
                                        Ok(msg) => AgentCommandResponse {
                                            success: true,
                                            message: msg,
                                        },
                                        Err(err) => AgentCommandResponse {
                                            success: false,
                                            message: err,
                                        },
                                    };
                                    let mut buf = Vec::new();
                                    if response.encode(&mut buf).is_ok() {
                                        let _ = client.publish(reply, buf.into()).await;
                                    }
                                }
                            }
                        });
                    }
                });

                // ── Registration (Implicit via periodic metrics) ─────────────
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));

                loop {
                    interval.tick().await;

                    let metrics = metrics_collector.collect().await;
                    tracing::info!(
                        "Collected metrics: cpu={:.2} ram={}/{} disk={}/{}",
                        metrics.cpu_usage,
                        metrics.ram_used_bytes,
                        metrics.ram_total_bytes,
                        metrics.disk_used_bytes,
                        metrics.disk_total_bytes,
                    );

                    use mikrom_proto::scheduler::{ReportMetricsRequest, WorkerHeartbeat};

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
                            apps_count: 0, // TODO: Count running VMs
                            timestamp: chrono::Utc::now().timestamp(),
                            load_avg_1: 0.0,
                            load_avg_5: 0.0,
                            load_avg_15: 0.0,
                            vms: std::collections::HashMap::new(), // TODO: Map VM metrics
                        }),
                    };

                    let mut payload = Vec::new();
                    if heartbeat.encode(&mut payload).is_ok() {
                        let res = client
                            .publish("mikrom.scheduler.worker.heartbeat", payload.into())
                            .await;
                        if let Err(e) = res {
                            tracing::error!("Failed to publish heartbeat to NATS: {}", e);
                            nats_client = None; // Force reconnect
                            break;
                        }
                    }

                    tracing::info!("Published heartbeat/metrics to NATS for {}", host_id);
                }
            }
        });

        // Wait for shutdown flag or keep alive
        loop {
            if *self.shutdown_flag.read() {
                tracing::info!("Agent shutdown requested");
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }

        Ok(())
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
