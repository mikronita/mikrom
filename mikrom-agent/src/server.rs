use crate::hypervisor::{HypervisorType, VmConfig, VmHypervisor, VmStatus, Volume};
use crate::metrics::MetricsCollector;
use crate::subjects;
use mikrom_agent_ebpf_common::{Action, Protocol};
use mikrom_proto::id::{AppId, VmId};
use parking_lot::RwLock;
use prost::Message;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

async fn publish_best_effort(
    nats: &async_nats::Client,
    subject: impl Into<String>,
    payload: Vec<u8>,
    context: &'static str,
) {
    let subject = subject.into();
    if let Err(e) = nats.publish(subject.clone(), payload.into()).await {
        warn!(
            %context,
            %subject,
            error = %e,
            "Failed to publish NATS message"
        );
    }
}

async fn encode_and_publish_best_effort<T: Message>(
    nats: &async_nats::Client,
    reply: async_nats::Subject,
    response: &T,
    context: &'static str,
) {
    let mut buf = Vec::new();
    if let Err(e) = response.encode(&mut buf) {
        warn!(
            %context,
            reply = %reply,
            error = %e,
            "Failed to encode NATS reply"
        );
        return;
    }

    publish_best_effort(nats, reply.to_string(), buf, context).await;
}

fn is_flapping_nats_session(uptime: Option<Duration>, threshold_secs: u64) -> bool {
    uptime.is_some_and(|age| age < Duration::from_secs(threshold_secs))
}

fn parse_vm_id(vm_id: &str) -> anyhow::Result<VmId> {
    VmId::from_str(vm_id).map_err(|e| anyhow::anyhow!("Invalid VM ID: {e}"))
}

fn abort_nats_listeners(
    cmd_handle: &mut tokio::task::JoinHandle<()>,
    health_check_handle: &mut tokio::task::JoinHandle<()>,
    heartbeat_handle: &mut tokio::task::JoinHandle<()>,
    mesh_handle: &mut tokio::task::JoinHandle<()>,
) {
    cmd_handle.abort();
    health_check_handle.abort();
    heartbeat_handle.abort();
    mesh_handle.abort();
}

pub struct AgentServer {
    config: crate::config::AgentConfig,
    ip_address: String,
    metrics_collector: MetricsCollector,
    hypervisors: Arc<HashMap<HypervisorType, Arc<dyn VmHypervisor>>>,
    shutdown_flag: Arc<RwLock<bool>>,
    http_client: reqwest::Client,
    wg_manager: Arc<mikrom_network::WireGuardManager>,
}

impl AgentServer {
    pub async fn new(config: crate::config::AgentConfig, ip_address: String) -> Self {
        let hypervisors = Arc::new(crate::hypervisor::factory::create_hypervisors(&config).await);
        Self::with_hypervisors(config, ip_address, hypervisors)
    }

    #[must_use]
    pub fn with_hypervisors(
        config: crate::config::AgentConfig,
        ip_address: String,
        hypervisors: Arc<HashMap<HypervisorType, Arc<dyn VmHypervisor>>>,
    ) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(config.cloud_hypervisor_configure_client_timeout())
            .build()
            .unwrap_or_default();

        let mut wg_manager = mikrom_network::WireGuardManager::new("wg-mikrom");
        if let Some(port) = config.wireguard_port {
            wg_manager = wg_manager.with_listen_port(port);
        } else {
            wg_manager = wg_manager.with_listen_port(51823);
        }

        Self {
            metrics_collector: MetricsCollector::with_hypervisors(hypervisors.clone()),
            hypervisors,
            config: config.clone(),
            ip_address,
            shutdown_flag: Arc::new(RwLock::new(false)),
            http_client,
            wg_manager: Arc::new(wg_manager),
        }
    }

    fn data_dir(&self) -> String {
        self.config.data_path.to_string_lossy().to_string()
    }

    pub async fn serve(&self) -> anyhow::Result<()> {
        if let Err(e) = crate::network::cleanup_host_networking().await {
            warn!("Failed to clean up stale NAT64 networking before startup: {e}");
        }

        for hv in (*self.hypervisors).values() {
            if let Err(e) = hv.init_network().await {
                error!("Failed to initialize host networking: {e}");
            }
            if let Err(e) = hv.load_runtime_state().await {
                warn!("Failed to loaded hypervisor state: {e}");
            }
            hv.cleanup_all_stale_resources().await;
            hv.start_background_tasks();
        }

        let priv_key = match self.config.get_wg_private_key() {
            Some(key) => key,
            None => {
                info!("WireGuard private key not provided, attempting to load or generate...");
                mikrom_network::KeyManager::load_or_generate_key(
                    &self.data_dir(),
                    &mikrom_network::FileWireGuardKeyStore,
                )
                .await
                .map_err(|e| anyhow::anyhow!("Failed to manage WireGuard keys: {e}"))?
            },
        };

        if let Err(e) = self.wg_manager.init(&priv_key, &self.config.host_id).await {
            error!("Failed to initialize WireGuard: {e:?}");
        }

        let pub_key = mikrom_network::KeyManager::get_public_key(&priv_key)
            .map_err(|e| anyhow::anyhow!("Failed to derive public key: {e}"))?;

        let http_server = crate::http::AgentHttpServer::new(
            self.config.http_port,
            self.metrics_collector.clone(),
            self.hypervisors.clone(),
        );
        let _http_handle = http_server.spawn();

        let nats_url = self.config.nats_url.clone();
        let nats_flapping_session_secs = self.config.nats_flapping_session_secs;
        let self_clone = self.clone();

        tokio::spawn(async move {
            let mut nats_client = None;
            let mut nats_session_started_at: Option<Instant> = None;
            let mut consecutive_failures: u32 = 0;
            let max_backoff = self_clone.config.nats_max_backoff();
            let circuit_breaker_threshold: u32 = 10;
            let circuit_breaker_cooldown = self_clone.config.nats_circuit_breaker_cooldown();
            let nats_connect_timeout = self_clone.config.nats_connect_timeout();

            loop {
                if nats_client.is_none() {
                    if consecutive_failures >= circuit_breaker_threshold {
                        error!(
                            cooldown_secs = circuit_breaker_cooldown.as_secs(),
                            "NATS circuit breaker triggered; cooling down"
                        );
                        tokio::time::sleep(circuit_breaker_cooldown).await;
                        consecutive_failures = 0;
                        continue;
                    }

                    info!(url = %nats_url, "Connecting to NATS...");
                    match tokio::time::timeout(
                        nats_connect_timeout,
                        async_nats::connect(nats_url.clone()),
                    )
                    .await
                    {
                        Ok(Ok(client)) => {
                            info!("Connected to NATS");
                            nats_client = Some(client);
                            nats_session_started_at = Some(Instant::now());
                            consecutive_failures = 0;
                        },
                        Ok(Err(e)) => {
                            consecutive_failures += 1;
                            let exponent = consecutive_failures.min(10);
                            let backoff = Duration::from_secs(
                                (2u64.pow(exponent)).min(max_backoff.as_secs()),
                            );
                            error!(
                                error = %e,
                                backoff_secs = backoff.as_secs(),
                                "Failed to connect to NATS; retrying"
                            );
                            tokio::time::sleep(backoff).await;
                            continue;
                        },
                        Err(e) => {
                            consecutive_failures += 1;
                            let exponent = consecutive_failures.min(10);
                            let backoff = Duration::from_secs(
                                (2u64.pow(exponent)).min(max_backoff.as_secs()),
                            );
                            error!(
                                error = %e,
                                backoff_secs = backoff.as_secs(),
                                "Timed out connecting to NATS; retrying"
                            );
                            tokio::time::sleep(backoff).await;
                            continue;
                        },
                    }
                }

                let client = match nats_client.as_ref() {
                    Some(c) => c.clone(),
                    None => continue,
                };

                for hv in self_clone.hypervisors.values() {
                    hv.set_nats_client(client.clone()).await;
                }

                let mut cmd_handle = self_clone.start_command_listener(client.clone());
                let mut health_check_handle =
                    self_clone.start_health_check_listener(client.clone());
                let mut heartbeat_handle = self_clone.start_heartbeat_loop(
                    client.clone(),
                    self_clone.config.host_id.clone(),
                    self_clone.config.hostname(),
                    pub_key.clone(),
                    self_clone
                        .wg_manager
                        .get_host_ipv6(&self_clone.config.host_id)
                        .to_string(),
                    i32::from(self_clone.wg_manager.listen_port()),
                    self_clone
                        .config
                        .agent_advertise_address
                        .clone()
                        .unwrap_or_else(|| self_clone.config.hostname()),
                    self_clone.config.get_supported_hypervisors(),
                );
                let mut mesh_handle = self_clone.start_mesh_listener(
                    client.clone(),
                    self_clone.config.host_id.clone(),
                    priv_key.clone(),
                );

                tokio::select! {
                    _ = &mut cmd_handle => {
                        warn!("NATS command listener exited unexpectedly");
                        abort_nats_listeners(&mut cmd_handle, &mut health_check_handle, &mut heartbeat_handle, &mut mesh_handle);
                    },
                    _ = &mut health_check_handle => {
                        warn!("NATS health check listener exited unexpectedly");
                        abort_nats_listeners(&mut cmd_handle, &mut health_check_handle, &mut heartbeat_handle, &mut mesh_handle);
                    },
                    _ = &mut heartbeat_handle => {
                        warn!("NATS heartbeat loop exited unexpectedly");
                        abort_nats_listeners(&mut cmd_handle, &mut health_check_handle, &mut heartbeat_handle, &mut mesh_handle);
                    },
                    _ = &mut mesh_handle => {
                        warn!("NATS mesh listener exited unexpectedly");
                        abort_nats_listeners(&mut cmd_handle, &mut health_check_handle, &mut heartbeat_handle, &mut mesh_handle);
                    },
                }

                let session_duration = nats_session_started_at.map(|t| t.elapsed());
                if is_flapping_nats_session(session_duration, nats_flapping_session_secs) {
                    consecutive_failures += 1;
                    warn!(
                        consecutive_failures,
                        "NATS session was too short (flapping detected)"
                    );
                } else {
                    consecutive_failures = 0;
                }

                nats_client = None;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });

        while !*self.shutdown_flag.read() {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        info!("Agent shutdown complete");
        Ok(())
    }

    fn start_mesh_listener(
        &self,
        client: async_nats::Client,
        host_id: String,
        priv_key: String,
    ) -> tokio::task::JoinHandle<()> {
        let wg_manager = self.wg_manager.clone();
        let host_subject = subjects::mesh_updates(&host_id);

        tokio::spawn(async move {
            let mut host_sub = match client.subscribe(host_subject.clone()).await {
                Ok(sub) => sub,
                Err(e) => {
                    error!("Failed to subscribe to host mesh updates: {e}");
                    return;
                },
            };

            info!("Listening for mesh updates on {}", host_subject);
            use futures::StreamExt;
            while let Some(msg) = host_sub.next().await {
                if let Ok(update) =
                    mikrom_proto::scheduler::NetworkMeshUpdate::decode(&msg.payload[..])
                {
                    debug!("Received mesh update with {} peers", update.peers.len());
                    if let Err(e) = wg_manager
                        .update_peers(&update.peers, &priv_key, &host_id)
                        .await
                    {
                        error!("Failed to update WireGuard peers: {e}");
                    }
                }
            }
        })
    }

    fn start_command_listener(&self, client: async_nats::Client) -> tokio::task::JoinHandle<()> {
        let self_clone = self.clone();
        let host_id = self.config.host_id.clone();
        let subject = subjects::agent_command(&host_id);

        tokio::spawn(async move {
            let mut cmd_sub = match client.subscribe(subject.clone()).await {
                Ok(sub) => sub,
                Err(e) => {
                    error!("Failed to subscribe to agent commands: {e}");
                    return;
                },
            };

            info!("Listening for agent commands on {}", subject);
            use futures::StreamExt;
            while let Some(msg) = cmd_sub.next().await {
                let self_inner = self_clone.clone();
                let client_inner = client.clone();
                tokio::spawn(async move {
                    if let Err(e) = self_inner.handle_command(&client_inner, msg).await {
                        error!("Failed to handle command: {e}");
                    }
                });
            }
        })
    }

    fn start_health_check_listener(
        &self,
        client: async_nats::Client,
    ) -> tokio::task::JoinHandle<()> {
        let host_id = self.config.host_id.clone();
        let subject = subjects::agent_health_check(&host_id);

        tokio::spawn(async move {
            let mut sub = match client.subscribe(subject.clone()).await {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to subscribe to health checks: {e}");
                    return;
                },
            };

            info!("Listening for health checks on {}", subject);
            use futures::StreamExt;
            while let Some(msg) = sub.next().await {
                if let Some(reply) = msg.reply {
                    let response = mikrom_proto::agent::CheckHealthResponse {
                        is_healthy: true,
                        message: "Agent is online".to_string(),
                    };
                    encode_and_publish_best_effort(&client, reply, &response, "health-check").await;
                }
            }
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn start_heartbeat_loop(
        &self,
        client: async_nats::Client,
        host_id: String,
        hostname: String,
        wireguard_pubkey: String,
        wireguard_ip: String,
        wireguard_port: i32,
        advertise_address: String,
        supported_hypervisors: Vec<i32>,
    ) -> tokio::task::JoinHandle<()> {
        let metrics_collector = self.metrics_collector.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(10));
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
                                    VmStatus::Starting => ProtoVmStatus::Starting as i32,
                                    VmStatus::Running => ProtoVmStatus::Running as i32,
                                    VmStatus::Paused => ProtoVmStatus::Paused as i32,
                                    VmStatus::Stopping => ProtoVmStatus::Stopping as i32,
                                    VmStatus::Stopped => ProtoVmStatus::Stopped as i32,
                                    VmStatus::Failed => ProtoVmStatus::Failed as i32,
                                },
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
                    wireguard_port,
                    advertise_address: advertise_address.clone(),
                    supported_hypervisors: supported_hypervisors.clone(),
                };

                let mut buf = Vec::new();
                if heartbeat.encode(&mut buf).is_ok() {
                    publish_best_effort(
                        &client,
                        subjects::SCHEDULER_WORKER_HEARTBEAT,
                        buf,
                        "worker-heartbeat",
                    )
                    .await;
                }
            }
        })
    }
}

impl Clone for AgentServer {
    fn clone(&self) -> Self {
        Self {
            metrics_collector: self.metrics_collector.clone(),
            hypervisors: self.hypervisors.clone(),
            config: self.config.clone(),
            ip_address: self.ip_address.clone(),
            shutdown_flag: self.shutdown_flag.clone(),
            http_client: self.http_client.clone(),
            wg_manager: self.wg_manager.clone(),
        }
    }
}

impl AgentServer {
    pub async fn trigger_shutdown(&self) {
        if let Err(e) = crate::network::cleanup_host_networking().await {
            warn!("Failed to clean up NAT64 networking during shutdown: {e}");
        }
        let mut shutdown = self.shutdown_flag.write();
        *shutdown = true;
    }

    async fn handle_command(
        &self,
        client: &async_nats::Client,
        msg: async_nats::Message,
    ) -> anyhow::Result<()> {
        use mikrom_proto::agent::{
            AgentCommand, AttachVolumeResponse, CancelMigrationResponse, DeleteVmResponse,
            DetachVolumeResponse, PauseVmResponse, QueryBalloonResponse, QueryMigrationResponse,
            ResumeVmResponse, SetBalloonResponse, StartMigrationResponse, StartVmResponse,
            StopVmResponse, UpdateFirewallResponse, VmSnapshotCreateResponse,
            VmSnapshotDeleteResponse, VmSnapshotListResponse, VmSnapshotRestoreResponse,
            agent_command::Command,
        };

        let cmd = AgentCommand::decode(&msg.payload[..])?;
        let reply = msg
            .reply
            .ok_or_else(|| anyhow::anyhow!("Command missing reply subject"))?;

        match cmd.command {
            Some(Command::StartVm(req)) => {
                let hv =
                    self.get_hypervisor(req.config.as_ref().map(|c| c.hypervisor).unwrap_or(0))?;
                let vm_id = VmId::from_str(&req.vm_id)
                    .map_err(|e| anyhow::anyhow!("Invalid VM ID: {e}"))?;
                let app_id = AppId::from_str(&req.app_id)
                    .map_err(|e| anyhow::anyhow!("Invalid App ID: {e}"))?;
                let config = self.map_proto_config(req.config.as_ref().unwrap());
                let res = hv.start_vm(vm_id, app_id, req.image, config).await;
                let response = StartVmResponse {
                    success: res.is_ok(),
                    vm_id: req.vm_id,
                    message: res.err().map(|e| e.to_string()).unwrap_or_default(),
                };
                encode_and_publish_best_effort(client, reply, &response, "start-vm-response").await;
            },
            Some(Command::StopVm(req)) => {
                let hv = self.get_hypervisor_for_vm(&req.vm_id).await?;
                let res = hv
                    .stop_vm(&VmId::from_str(&req.vm_id).unwrap_or_default())
                    .await;
                let response = StopVmResponse {
                    success: res.is_ok(),
                    message: res.err().map(|e| e.to_string()).unwrap_or_default(),
                };
                encode_and_publish_best_effort(client, reply, &response, "stop-vm-response").await;
            },
            Some(Command::PauseVm(req)) => {
                let hv = self.get_hypervisor_for_vm(&req.vm_id).await?;
                let res = hv
                    .pause_vm(&VmId::from_str(&req.vm_id).unwrap_or_default())
                    .await;
                let response = PauseVmResponse {
                    success: res.is_ok(),
                    message: res.err().map(|e| e.to_string()).unwrap_or_default(),
                };
                encode_and_publish_best_effort(client, reply, &response, "pause-vm-response").await;
            },
            Some(Command::ResumeVm(req)) => {
                let hv = self.get_hypervisor_for_vm(&req.vm_id).await?;
                let res = hv
                    .resume_vm(&VmId::from_str(&req.vm_id).unwrap_or_default())
                    .await;
                let response = ResumeVmResponse {
                    success: res.is_ok(),
                    message: res.err().map(|e| e.to_string()).unwrap_or_default(),
                };
                encode_and_publish_best_effort(client, reply, &response, "resume-vm-response")
                    .await;
            },
            Some(Command::DeleteVm(req)) => {
                let hv = self.get_hypervisor(req.hypervisor)?;
                let vm_id = parse_vm_id(&req.vm_id)?;
                let res = hv.delete_vm(&vm_id).await;
                let response = DeleteVmResponse {
                    success: res.is_ok(),
                    message: res.err().map(|e| e.to_string()).unwrap_or_default(),
                };
                encode_and_publish_best_effort(client, reply, &response, "delete-vm-response")
                    .await;
            },
            Some(Command::UpdateFirewall(req)) => {
                let hv = self.get_hypervisor_for_vm(&req.vm_id).await?;
                let rules = req
                    .rules
                    .iter()
                    .map(|r| mikrom_agent_ebpf_common::FirewallRule {
                        protocol: match r.protocol.to_lowercase().as_str() {
                            "tcp" => Protocol::Tcp,
                            "udp" => Protocol::Udp,
                            _ => Protocol::Any,
                        },
                        port_start: r.port_start as u16,
                        port_end: r.port_end as u16,
                        action: match r.action.to_lowercase().as_str() {
                            "allow" => Action::Allow,
                            _ => Action::Deny,
                        },
                        remote_ip: [0; 16],
                        remote_prefix: 0,
                    })
                    .collect();
                let res = hv
                    .update_vm_firewall(&VmId::from_str(&req.vm_id).unwrap_or_default(), rules)
                    .await;
                let response = UpdateFirewallResponse {
                    success: res.is_ok(),
                    message: res.err().map(|e| e.to_string()).unwrap_or_default(),
                };
                encode_and_publish_best_effort(
                    client,
                    reply,
                    &response,
                    "update-firewall-response",
                )
                .await;
            },
            Some(Command::CreateVolume(req)) => {
                let res = crate::ceph::CephRbd::create_volume(
                    &req.pool_name,
                    &req.volume_id,
                    req.size_mib as i32,
                )
                .await;
                self.publish_generic_response(client, reply, res, "create-volume")
                    .await;
            },
            Some(Command::DeleteVolume(req)) => {
                let res = crate::ceph::CephRbd::delete_volume(&req.pool_name, &req.volume_id).await;
                self.publish_generic_response(client, reply, res, "delete-volume")
                    .await;
            },
            Some(Command::CreateSnapshot(req)) => {
                let res = crate::ceph::CephRbd::create_snapshot(
                    &req.pool_name,
                    &req.volume_id,
                    &req.snapshot_name,
                )
                .await;
                self.publish_generic_response(client, reply, res, "create-snapshot")
                    .await;
            },
            Some(Command::DeleteSnapshot(req)) => {
                let res = crate::ceph::CephRbd::delete_snapshot(
                    &req.pool_name,
                    &req.volume_id,
                    &req.snapshot_name,
                )
                .await;
                self.publish_generic_response(client, reply, res, "delete-snapshot")
                    .await;
            },
            Some(Command::RestoreSnapshot(req)) => {
                let res = crate::ceph::CephRbd::restore_snapshot(
                    &req.pool_name,
                    &req.volume_id,
                    &req.snapshot_name,
                )
                .await;
                self.publish_generic_response(client, reply, res, "restore-snapshot")
                    .await;
            },
            Some(Command::CloneVolume(req)) => {
                let res = crate::ceph::CephRbd::clone_volume(
                    &req.pool_name,
                    &req.source_volume_id,
                    &req.snapshot_name,
                    &req.target_volume_id,
                )
                .await;
                self.publish_generic_response(client, reply, res, "clone-volume")
                    .await;
            },
            Some(Command::VmSnapshotCreate(req)) => {
                let hv = self.get_hypervisor_for_vm(&req.vm_id).await?;
                let res = hv
                    .create_vm_snapshot(
                        &VmId::from_str(&req.vm_id).unwrap_or_default(),
                        &req.snapshot_name,
                    )
                    .await;
                let response = VmSnapshotCreateResponse {
                    success: res.is_ok(),
                    message: res.err().map(|e| e.to_string()).unwrap_or_default(),
                };
                encode_and_publish_best_effort(client, reply, &response, "vm-snapshot-create")
                    .await;
            },
            Some(Command::VmSnapshotRestore(req)) => {
                let hv = self.get_hypervisor_for_vm(&req.vm_id).await?;
                let res = hv
                    .restore_vm_snapshot(
                        &VmId::from_str(&req.vm_id).unwrap_or_default(),
                        &req.snapshot_name,
                    )
                    .await;
                let response = VmSnapshotRestoreResponse {
                    success: res.is_ok(),
                    message: res.err().map(|e| e.to_string()).unwrap_or_default(),
                };
                encode_and_publish_best_effort(client, reply, &response, "vm-snapshot-restore")
                    .await;
            },
            Some(Command::VmSnapshotDelete(req)) => {
                let hv = self.get_hypervisor_for_vm(&req.vm_id).await?;
                let res = hv
                    .delete_vm_snapshot(
                        &VmId::from_str(&req.vm_id).unwrap_or_default(),
                        &req.snapshot_name,
                    )
                    .await;
                let response = VmSnapshotDeleteResponse {
                    success: res.is_ok(),
                    message: res.err().map(|e| e.to_string()).unwrap_or_default(),
                };
                encode_and_publish_best_effort(client, reply, &response, "vm-snapshot-delete")
                    .await;
            },
            Some(Command::VmSnapshotList(req)) => {
                let hv = self.get_hypervisor_for_vm(&req.vm_id).await?;
                let res = hv
                    .list_vm_snapshots(&VmId::from_str(&req.vm_id).unwrap_or_default())
                    .await;
                let response = VmSnapshotListResponse {
                    success: res.is_ok(),
                    message: res
                        .as_ref()
                        .err()
                        .map(|e| e.to_string())
                        .unwrap_or_default(),
                    snapshots: res.unwrap_or_default(),
                };
                encode_and_publish_best_effort(client, reply, &response, "vm-snapshot-list").await;
            },
            Some(Command::AttachVolume(req)) => {
                let hv = self.get_hypervisor_for_vm(&req.vm_id).await?;
                let res = hv
                    .attach_volume(
                        &VmId::from_str(&req.vm_id).unwrap_or_default(),
                        &req.volume_id,
                        &req.mount_point,
                        req.read_only,
                    )
                    .await;
                let response = AttachVolumeResponse {
                    success: res.is_ok(),
                    message: res.err().map(|e| e.to_string()).unwrap_or_default(),
                };
                encode_and_publish_best_effort(client, reply, &response, "attach-volume").await;
            },
            Some(Command::DetachVolume(req)) => {
                let hv = self.get_hypervisor_for_vm(&req.vm_id).await?;
                let res = hv
                    .detach_volume(
                        &VmId::from_str(&req.vm_id).unwrap_or_default(),
                        &req.volume_id,
                    )
                    .await;
                let response = DetachVolumeResponse {
                    success: res.is_ok(),
                    message: res.err().map(|e| e.to_string()).unwrap_or_default(),
                };
                encode_and_publish_best_effort(client, reply, &response, "detach-volume").await;
            },
            Some(Command::StartMigration(req)) => {
                let hv = self.get_hypervisor_for_vm(&req.vm_id).await?;
                let res = hv
                    .start_migration(
                        &VmId::from_str(&req.vm_id).unwrap_or_default(),
                        &req.target_host,
                        &req.target_uri,
                    )
                    .await;
                let response = StartMigrationResponse {
                    success: res.is_ok(),
                    message: res.err().map(|e| e.to_string()).unwrap_or_default(),
                };
                encode_and_publish_best_effort(client, reply, &response, "start-migration").await;
            },
            Some(Command::CancelMigration(req)) => {
                let hv = self.get_hypervisor_for_vm(&req.vm_id).await?;
                let res = hv
                    .cancel_migration(&VmId::from_str(&req.vm_id).unwrap_or_default())
                    .await;
                let response = CancelMigrationResponse {
                    success: res.is_ok(),
                    message: res.err().map(|e| e.to_string()).unwrap_or_default(),
                };
                encode_and_publish_best_effort(client, reply, &response, "cancel-migration").await;
            },
            Some(Command::QueryMigration(req)) => {
                let hv = self.get_hypervisor_for_vm(&req.vm_id).await?;
                let res = hv
                    .query_migration(&VmId::from_str(&req.vm_id).unwrap_or_default())
                    .await;
                let response = QueryMigrationResponse {
                    success: res.is_ok(),
                    message: res
                        .as_ref()
                        .err()
                        .map(|e| e.to_string())
                        .unwrap_or_default(),
                    status: res.unwrap_or_default(),
                    total_bytes: 0,
                    transferred_bytes: 0,
                    remaining_bytes: 0,
                };
                encode_and_publish_best_effort(client, reply, &response, "query-migration").await;
            },
            Some(Command::SetBalloon(req)) => {
                let hv = self.get_hypervisor_for_vm(&req.vm_id).await?;
                let res = hv
                    .set_balloon_size(
                        &VmId::from_str(&req.vm_id).unwrap_or_default(),
                        req.target_memory_mib,
                    )
                    .await;
                let response = SetBalloonResponse {
                    success: res.is_ok(),
                    message: res.err().map(|e| e.to_string()).unwrap_or_default(),
                };
                encode_and_publish_best_effort(client, reply, &response, "set-balloon").await;
            },
            Some(Command::QueryBalloon(req)) => {
                let hv = self.get_hypervisor_for_vm(&req.vm_id).await?;
                let res = hv
                    .query_balloon(&VmId::from_str(&req.vm_id).unwrap_or_default())
                    .await;
                let success = res.is_ok();
                let (actual, max) = res.unwrap_or((0, 0));
                let response = QueryBalloonResponse {
                    success,
                    message: String::new(), // Success message or could extract from Err if failed
                    actual_memory_mib: actual,
                    max_memory_mib: max,
                };
                encode_and_publish_best_effort(client, reply, &response, "query-balloon").await;
            },
            _ => error!("Received AgentCommand with unsupported or empty command variant"),
        }

        Ok(())
    }

    fn get_hypervisor(&self, hv_type: i32) -> anyhow::Result<Arc<dyn VmHypervisor>> {
        let proto_type = match hv_type {
            1 => HypervisorType::Firecracker,
            3 => HypervisorType::CloudHypervisor,
            _ => return Err(anyhow::anyhow!("Unsupported hypervisor type: {}", hv_type)),
        };

        self.hypervisors
            .get(&proto_type)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Hypervisor {:?} not initialized", proto_type))
    }

    async fn get_hypervisor_for_vm(&self, vm_id: &str) -> anyhow::Result<Arc<dyn VmHypervisor>> {
        let vid = VmId::from_str(vm_id).map_err(|e| anyhow::anyhow!("Invalid VM ID: {e}"))?;
        for hv in self.hypervisors.values() {
            if hv.get_vm_info(&vid).await.is_some() {
                return Ok(hv.clone());
            }
        }
        Err(anyhow::anyhow!("VM {} not found on any hypervisor", vm_id))
    }

    async fn publish_generic_response(
        &self,
        client: &async_nats::Client,
        reply: async_nats::Subject,
        res: anyhow::Result<()>,
        ctx: &'static str,
    ) {
        let response = mikrom_proto::agent::AgentCommandResponse {
            success: res.is_ok(),
            message: res.err().map(|e| e.to_string()).unwrap_or_default(),
        };
        encode_and_publish_best_effort(client, reply, &response, ctx).await;
    }

    fn map_proto_config(&self, proto: &mikrom_proto::agent::VmConfig) -> VmConfig {
        VmConfig {
            vcpus: proto.vcpus,
            memory_mib: proto.memory_mib as u64,
            disk_mib: proto.disk_mib as u64,
            env: proto.env.clone(),
            volumes: proto
                .volumes
                .iter()
                .map(|v| Volume {
                    volume_id: v.volume_id.clone(),
                    size_mib: v.size_mib,
                    read_only: v.read_only,
                    pool_name: v.pool_name.clone(),
                    mount_point: v.mount_point.clone(),
                    access_mode: v.access_mode,
                })
                .collect(),
            port: proto.port,
            health_check_path: proto.health_check_path.clone(),
            ipv6_address: Some(proto.ipv6_address.clone()),
            ipv6_gateway: Some(proto.ipv6_gateway.clone()),
            mac_address: None,
            netmask: None,
            gateway: None,
            ip_address: None,
            workload_type: proto.workload_type,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_vm_id;

    #[test]
    fn parse_vm_id_rejects_invalid_values() {
        assert!(parse_vm_id("not-a-uuid").is_err());
    }
}
