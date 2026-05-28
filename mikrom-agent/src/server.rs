use crate::ceph::StorageProvider;
use crate::hypervisor::{
    HypervisorError, HypervisorType, VmConfig, VmHypervisor, VmInfo, VmStatus, Volume,
};
use crate::metrics::MetricsCollector;
use crate::subjects;
use parking_lot::RwLock;
use prost::Message;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::info;

async fn publish_best_effort(
    nats: &async_nats::Client,
    subject: impl Into<String>,
    payload: Vec<u8>,
    context: &'static str,
) {
    let subject = subject.into();
    if let Err(e) = nats.publish(subject.clone(), payload.into()).await {
        tracing::warn!(
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
        tracing::warn!(
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
    wg_manager: Arc<crate::wireguard::WireGuardManager>,
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
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap_or_default();

        let mut wg_manager = crate::wireguard::WireGuardManager::new("wg0");
        if let Some(port) = config.wireguard_port {
            wg_manager = wg_manager.with_listen_port(port);
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

    /// Returns the data directory used by hypervisors for persistence.
    fn data_dir(&self) -> String {
        self.config.data_path.to_string_lossy().to_string()
    }

    pub async fn serve(&self) -> anyhow::Result<()> {
        // Initialize all hypervisors (networking, state loading, GC, background tasks)
        for hv in (*self.hypervisors).values() {
            if let Err(e) = hv.init_network().await {
                tracing::error!("Failed to initialize host networking: {e}");
            }

            if let Err(e) = hv.load_runtime_state().await {
                tracing::warn!("Failed to loaded hypervisor state: {e}");
            }

            hv.cleanup_all_stale_resources().await;
            hv.start_background_tasks();
        }

        // 3. Initialize WireGuard
        let priv_key = match self.config.get_wg_private_key() {
            Some(key) => key,
            None => {
                info!("WireGuard private key not provided, attempting to load or generate...");
                self.wg_manager
                    .load_or_generate_key(&self.data_dir())
                    .await?
            },
        };

        if let Err(e) = self.wg_manager.init(&priv_key, &self.config.host_id).await {
            tracing::error!("Failed to initialize WireGuard: {e:?}");
        }

        let pub_key = self.wg_manager.get_public_key(&priv_key)?;

        // 4. Spawn HTTP health/metrics server
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
            let max_backoff_secs: u64 = 60;
            let circuit_breaker_threshold: u32 = 10;
            let circuit_breaker_cooldown_secs: u64 = 300;

            loop {
                if nats_client.is_none() {
                    // Circuit breaker: if too many failures, wait longer before retrying
                    if consecutive_failures >= circuit_breaker_threshold {
                        tracing::warn!(
                            failures = consecutive_failures,
                            cooldown_secs = circuit_breaker_cooldown_secs,
                            "NATS circuit breaker open, cooling down before reconnect"
                        );
                        tokio::time::sleep(tokio::time::Duration::from_secs(
                            circuit_breaker_cooldown_secs,
                        ))
                        .await;
                        consecutive_failures = 0;
                    }

                    tracing::info!("Connecting to NATS at {nats_url}");
                    match async_nats::connect(&nats_url).await {
                        Ok(client) => {
                            tracing::info!("Connected to NATS");
                            nats_client = Some(client.clone());
                            nats_session_started_at = Some(Instant::now());
                            consecutive_failures = 0;
                            // Set NATS client on all hypervisors
                            for hv in (*self_clone.hypervisors).values() {
                                hv.set_nats_client(client.clone()).await;
                            }
                        },
                        Err(e) => {
                            consecutive_failures += 1;
                            let delay = std::cmp::min(
                                2u64.saturating_pow(consecutive_failures),
                                max_backoff_secs,
                            );
                            // Add jitter (0-25%) to avoid thundering herd
                            let jitter = rand::random::<u64>() % (delay / 4 + 1);
                            let total_delay = delay + jitter;

                            tracing::error!(
                                failures = consecutive_failures,
                                delay_secs = total_delay,
                                "Failed to connect to NATS: {e}. Retrying..."
                            );
                            tokio::time::sleep(tokio::time::Duration::from_secs(total_delay)).await;
                            continue;
                        },
                    }
                }

                let Some(client) = nats_client.as_ref() else {
                    continue;
                };

                // 1. Spawn listeners
                let mut cmd_handle = self_clone.start_command_listener(client.clone());
                let mut health_check_handle =
                    self_clone.start_health_check_listener(client.clone());
                let mut heartbeat_handle =
                    self_clone.start_heartbeat_loop(client.clone(), pub_key.clone());
                let mut mesh_handle = self_clone.start_mesh_listener(
                    client.clone(),
                    self_clone.config.host_id.clone(),
                    priv_key.clone(),
                );

                tokio::select! {
                    _ = &mut cmd_handle => {
                        let uptime = nats_session_started_at.map(|started| started.elapsed());
                        let flapping = is_flapping_nats_session(uptime, nats_flapping_session_secs);
                        tracing::warn!(
                            uptime_secs = uptime.map(|d| d.as_secs()).unwrap_or_default(),
                            flapping,
                            "Command listener exited; reconnecting NATS"
                        );
                        if flapping {
                            consecutive_failures = consecutive_failures.saturating_add(1);
                        } else {
                            consecutive_failures = 0;
                        }
                        abort_nats_listeners(
                            &mut cmd_handle,
                            &mut health_check_handle,
                            &mut heartbeat_handle,
                            &mut mesh_handle,
                        );
                        nats_client = None;
                        nats_session_started_at = None;
                    }
                    _ = &mut health_check_handle => {
                        let uptime = nats_session_started_at.map(|started| started.elapsed());
                        let flapping = is_flapping_nats_session(uptime, nats_flapping_session_secs);
                        tracing::warn!(
                            uptime_secs = uptime.map(|d| d.as_secs()).unwrap_or_default(),
                            flapping,
                            "Health check listener exited; reconnecting NATS"
                        );
                        if flapping {
                            consecutive_failures = consecutive_failures.saturating_add(1);
                        } else {
                            consecutive_failures = 0;
                        }
                        abort_nats_listeners(
                            &mut cmd_handle,
                            &mut health_check_handle,
                            &mut heartbeat_handle,
                            &mut mesh_handle,
                        );
                        nats_client = None;
                        nats_session_started_at = None;
                    }
                    _ = &mut heartbeat_handle => {
                        let uptime = nats_session_started_at.map(|started| started.elapsed());
                        let flapping = is_flapping_nats_session(uptime, nats_flapping_session_secs);
                        tracing::warn!(
                            uptime_secs = uptime.map(|d| d.as_secs()).unwrap_or_default(),
                            flapping,
                            "Heartbeat loop exited; reconnecting NATS"
                        );
                        if flapping {
                            consecutive_failures = consecutive_failures.saturating_add(1);
                        } else {
                            consecutive_failures = 0;
                        }
                        abort_nats_listeners(
                            &mut cmd_handle,
                            &mut health_check_handle,
                            &mut heartbeat_handle,
                            &mut mesh_handle,
                        );
                        nats_client = None;
                        nats_session_started_at = None;
                    }
                    _ = &mut mesh_handle => {
                        let uptime = nats_session_started_at.map(|started| started.elapsed());
                        let flapping = is_flapping_nats_session(uptime, nats_flapping_session_secs);
                        tracing::warn!(
                            uptime_secs = uptime.map(|d| d.as_secs()).unwrap_or_default(),
                            flapping,
                            "Mesh listener exited; reconnecting NATS"
                        );
                        if flapping {
                            consecutive_failures = consecutive_failures.saturating_add(1);
                        } else {
                            consecutive_failures = 0;
                        }
                        abort_nats_listeners(
                            &mut cmd_handle,
                            &mut health_check_handle,
                            &mut heartbeat_handle,
                            &mut mesh_handle,
                        );
                        nats_client = None;
                        nats_session_started_at = None;
                    }
                }
            }
        });

        // Wait for shutdown flag
        while !*self.shutdown_flag.read() {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }

        tracing::info!("Agent shutdown requested");
        self.shutdown().await;
        Ok(())
    }

    /// Trigger graceful shutdown from an external signal handler.
    pub async fn trigger_shutdown(&self) {
        tracing::info!("Shutdown signal received, initiating graceful shutdown");
        *self.shutdown_flag.write() = true;
    }

    /// Graceful shutdown sequence:
    /// 1. Persist runtime state for all hypervisors
    /// 2. Stop all running VMs
    /// 3. Log completion
    async fn shutdown(&self) {
        tracing::info!("Persisting runtime state...");
        for (htype, hv) in &*self.hypervisors {
            if let Err(e) = hv.persist_runtime_state().await {
                tracing::error!(hypervisor = ?htype, error = %e, "Failed to persist runtime state");
            }
        }

        tracing::info!("Stopping all VMs...");
        for (htype, hv) in &*self.hypervisors {
            let vms = hv.get_all_vms().await;
            for vm in vms {
                if matches!(
                    vm.status,
                    VmStatus::Running | VmStatus::Starting | VmStatus::Paused
                ) {
                    tracing::info!(
                        vm_id = %vm.vm_id,
                        hypervisor = ?htype,
                        status = ?vm.status,
                        "Stopping VM during shutdown"
                    );
                    if let Err(e) = hv.stop_vm(&vm.vm_id).await {
                        tracing::error!(
                            vm_id = %vm.vm_id,
                            error = %e,
                            "Failed to stop VM during shutdown"
                        );
                    }
                }
            }
        }

        tracing::info!("Agent shutdown complete");
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
                    tracing::debug!("Received mesh update with {} peers", update.peers.len());
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
        let self_clone = self.clone();
        let host_id = self.config.host_id.clone();
        let subject = subjects::agent_command(&host_id);

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
                self_clone.handle_nats_command(msg, &client).await;
            }
        })
    }

    fn start_health_check_listener(
        &self,
        client: async_nats::Client,
    ) -> tokio::task::JoinHandle<()> {
        let host_id = self.config.host_id.clone();
        let subject = subjects::agent_health_check(&host_id);
        let hypervisors = self.hypervisors.clone();
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
                Self::handle_health_check(msg, &hypervisors, &nats, &http_client).await;
            }
        })
    }

    async fn handle_nats_command(&self, message: async_nats::Message, nats: &async_nats::Client) {
        use mikrom_proto::agent::AgentCommand;
        let Ok(command) = AgentCommand::decode(&message.payload[..]) else {
            tracing::error!("Failed to decode AgentCommand");
            return;
        };

        let result = self.dispatch_agent_command(command.command).await;
        Self::reply_agent_command(message, nats, result).await;
    }

    /// Extract the VM ID from a command that targets a specific VM.
    fn extract_vm_id_from_command(
        command: &mikrom_proto::agent::agent_command::Command,
    ) -> Option<&str> {
        match command {
            mikrom_proto::agent::agent_command::Command::StartVm(req) => Some(&req.vm_id),
            mikrom_proto::agent::agent_command::Command::StopVm(req) => Some(&req.vm_id),
            mikrom_proto::agent::agent_command::Command::PauseVm(req) => Some(&req.vm_id),
            mikrom_proto::agent::agent_command::Command::ResumeVm(req) => Some(&req.vm_id),
            mikrom_proto::agent::agent_command::Command::DeleteVm(req) => Some(&req.vm_id),
            mikrom_proto::agent::agent_command::Command::UpdateFirewall(req) => Some(&req.vm_id),
            mikrom_proto::agent::agent_command::Command::VmSnapshotCreate(req) => Some(&req.vm_id),
            mikrom_proto::agent::agent_command::Command::VmSnapshotRestore(req) => Some(&req.vm_id),
            mikrom_proto::agent::agent_command::Command::VmSnapshotDelete(req) => Some(&req.vm_id),
            mikrom_proto::agent::agent_command::Command::VmSnapshotList(req) => Some(&req.vm_id),
            mikrom_proto::agent::agent_command::Command::AttachVolume(req) => Some(&req.vm_id),
            mikrom_proto::agent::agent_command::Command::DetachVolume(req) => Some(&req.vm_id),
            mikrom_proto::agent::agent_command::Command::StartMigration(req) => Some(&req.vm_id),
            mikrom_proto::agent::agent_command::Command::CancelMigration(req) => Some(&req.vm_id),
            mikrom_proto::agent::agent_command::Command::QueryMigration(req) => Some(&req.vm_id),
            mikrom_proto::agent::agent_command::Command::SetBalloon(req) => Some(&req.vm_id),
            mikrom_proto::agent::agent_command::Command::QueryBalloon(req) => Some(&req.vm_id),
            _ => None,
        }
    }

    /// Pick the right hypervisor for a StartVm command based on config.
    fn resolve_hypervisor_for_start<'a>(
        req: &mikrom_proto::agent::StartVmRequest,
        hypervisors: &'a HashMap<HypervisorType, Arc<dyn VmHypervisor>>,
    ) -> Option<&'a Arc<dyn VmHypervisor>> {
        let htype = match req.config.as_ref()?.hypervisor {
            0 | 1 => HypervisorType::Firecracker,
            2 => HypervisorType::QemuMicrovm,
            3 => HypervisorType::CloudHypervisor,
            _ => return None,
        };
        hypervisors.get(&htype)
    }

    /// Search every hypervisor for a VM by ID.
    async fn find_hypervisor_for_vm<'a>(
        vm_id: &mikrom_proto::id::VmId,
        hypervisors: &'a HashMap<HypervisorType, Arc<dyn VmHypervisor>>,
    ) -> Option<&'a Arc<dyn VmHypervisor>> {
        for hv in hypervisors.values() {
            if hv.get_vm_info(vm_id).await.is_some() {
                return Some(hv);
            }
        }
        None
    }

    /// Count active VMs across all hypervisors.
    async fn count_active_vms(
        hypervisors: &HashMap<HypervisorType, Arc<dyn VmHypervisor>>,
    ) -> usize {
        let mut total = 0;
        for hv in hypervisors.values() {
            total += hv
                .get_all_vms()
                .await
                .into_iter()
                .filter(|v| {
                    matches!(
                        v.status,
                        VmStatus::Running | VmStatus::Starting | VmStatus::Paused
                    )
                })
                .count();
        }
        total
    }

    async fn dispatch_agent_command(
        &self,
        command: Option<mikrom_proto::agent::agent_command::Command>,
    ) -> Result<Vec<u8>, HypervisorError> {
        let Some(cmd) = command else {
            return Err(HypervisorError::ProcessError("Empty command".to_string()));
        };

        let hypervisors = &*self.hypervisors;
        let hv = if let mikrom_proto::agent::agent_command::Command::StartVm(ref req) = cmd {
            Self::resolve_hypervisor_for_start(req, hypervisors)
        } else if let Some(vid) = Self::extract_vm_id_from_command(&cmd) {
            let vm_id = Self::parse_vm_id(vid)?;
            Self::find_hypervisor_for_vm(&vm_id, hypervisors).await
        } else {
            None
        };
        let hv =
            hv.ok_or_else(|| HypervisorError::ProcessError("No hypervisor available".to_string()))?;

        match cmd {
            mikrom_proto::agent::agent_command::Command::StartVm(req) => {
                self.handle_start_vm(hv, req, self.config.max_vms_per_host)
                    .await
            },
            mikrom_proto::agent::agent_command::Command::StopVm(req) => {
                self.handle_stop_vm(hv, req).await
            },
            mikrom_proto::agent::agent_command::Command::PauseVm(req) => {
                self.handle_pause_vm(hv, req).await
            },
            mikrom_proto::agent::agent_command::Command::ResumeVm(req) => {
                self.handle_resume_vm(hv, req).await
            },
            mikrom_proto::agent::agent_command::Command::DeleteVm(req) => {
                self.handle_delete_vm(hv, req).await
            },
            mikrom_proto::agent::agent_command::Command::UpdateFirewall(req) => {
                self.handle_update_firewall(hv, req).await
            },
            mikrom_proto::agent::agent_command::Command::CreateSnapshot(req) => {
                self.handle_create_snapshot(req).await
            },
            mikrom_proto::agent::agent_command::Command::DeleteVolume(req) => {
                self.handle_delete_volume(req).await
            },
            mikrom_proto::agent::agent_command::Command::DeleteSnapshot(req) => {
                self.handle_delete_snapshot(req).await
            },
            mikrom_proto::agent::agent_command::Command::CreateVolume(req) => {
                self.handle_create_volume(req).await
            },
            mikrom_proto::agent::agent_command::Command::RestoreSnapshot(req) => {
                self.handle_restore_snapshot(req).await
            },
            mikrom_proto::agent::agent_command::Command::CloneVolume(req) => {
                self.handle_clone_volume(req).await
            },
            mikrom_proto::agent::agent_command::Command::VmSnapshotCreate(req) => {
                self.handle_vm_snapshot_create(hv, req).await
            },
            mikrom_proto::agent::agent_command::Command::VmSnapshotRestore(req) => {
                self.handle_vm_snapshot_restore(hv, req).await
            },
            mikrom_proto::agent::agent_command::Command::VmSnapshotDelete(req) => {
                self.handle_vm_snapshot_delete(hv, req).await
            },
            mikrom_proto::agent::agent_command::Command::VmSnapshotList(req) => {
                self.handle_vm_snapshot_list(hv, req).await
            },
            mikrom_proto::agent::agent_command::Command::AttachVolume(req) => {
                self.handle_attach_volume(hv, req).await
            },
            mikrom_proto::agent::agent_command::Command::DetachVolume(req) => {
                self.handle_detach_volume(hv, req).await
            },
            mikrom_proto::agent::agent_command::Command::StartMigration(req) => {
                self.handle_start_migration(hv, req).await
            },
            mikrom_proto::agent::agent_command::Command::CancelMigration(req) => {
                self.handle_cancel_migration(hv, req).await
            },
            mikrom_proto::agent::agent_command::Command::QueryMigration(req) => {
                self.handle_query_migration(hv, req).await
            },
            mikrom_proto::agent::agent_command::Command::SetBalloon(req) => {
                self.handle_set_balloon(hv, req).await
            },
            mikrom_proto::agent::agent_command::Command::QueryBalloon(req) => {
                self.handle_query_balloon(hv, req).await
            },
        }
    }

    fn ok_response(msg: &str) -> Vec<u8> {
        let mut buf = Vec::new();
        if (mikrom_proto::agent::AgentCommandResponse {
            success: true,
            message: msg.to_string(),
        })
        .encode(&mut buf)
        .is_err()
        {
            tracing::warn!("Failed to encode successful AgentCommandResponse");
        }
        buf
    }

    async fn handle_start_vm(
        &self,
        hv: &Arc<dyn VmHypervisor>,
        req: mikrom_proto::agent::StartVmRequest,
        max_vms: u32,
    ) -> Result<Vec<u8>, HypervisorError> {
        if max_vms > 0 {
            let active = Self::count_active_vms(self.hypervisors.as_ref()).await;
            if active >= max_vms as usize {
                return Err(HypervisorError::ProcessError(format!(
                    "Host at VM capacity ({active}/{max_vms}) — cannot start new VM"
                )));
            }
        }

        let config = Self::proto_vm_config(req.config);
        let vm_id = Self::parse_vm_id(&req.vm_id)?;
        let app_id = Self::parse_app_id(&req.app_id)?;

        hv.start_vm(vm_id, app_id, req.image, config)
            .await
            .map(|_| Self::ok_response("VM started"))
    }

    async fn handle_stop_vm(
        &self,
        hv: &Arc<dyn VmHypervisor>,
        req: mikrom_proto::agent::StopVmRequest,
    ) -> Result<Vec<u8>, HypervisorError> {
        let vm_id = Self::parse_vm_id(&req.vm_id)?;
        hv.stop_vm(&vm_id)
            .await
            .map(|_| Self::ok_response("VM stopped"))
    }

    async fn handle_pause_vm(
        &self,
        hv: &Arc<dyn VmHypervisor>,
        req: mikrom_proto::agent::PauseVmRequest,
    ) -> Result<Vec<u8>, HypervisorError> {
        let vm_id = Self::parse_vm_id(&req.vm_id)?;
        hv.pause_vm(&vm_id)
            .await
            .map(|_| Self::ok_response("VM paused"))
    }

    async fn handle_resume_vm(
        &self,
        hv: &Arc<dyn VmHypervisor>,
        req: mikrom_proto::agent::ResumeVmRequest,
    ) -> Result<Vec<u8>, HypervisorError> {
        let vm_id = Self::parse_vm_id(&req.vm_id)?;
        hv.resume_vm(&vm_id)
            .await
            .map(|_| Self::ok_response("VM resumed"))
    }

    async fn handle_delete_vm(
        &self,
        hv: &Arc<dyn VmHypervisor>,
        req: mikrom_proto::agent::DeleteVmRequest,
    ) -> Result<Vec<u8>, HypervisorError> {
        let vm_id = Self::parse_vm_id(&req.vm_id)?;
        hv.delete_vm(&vm_id)
            .await
            .map(|_| Self::ok_response("VM resources purged"))
    }

    async fn handle_update_firewall(
        &self,
        hv: &Arc<dyn VmHypervisor>,
        req: mikrom_proto::agent::UpdateFirewallRequest,
    ) -> Result<Vec<u8>, HypervisorError> {
        let vm_id = Self::parse_vm_id(&req.vm_id)?;
        let rules = Self::map_firewall_rules(req.rules);

        hv.update_vm_firewall(&vm_id, rules)
            .await
            .map(|_| Self::ok_response("Firewall rules updated"))
    }

    async fn handle_create_snapshot(
        &self,
        req: mikrom_proto::agent::CreateSnapshotRequest,
    ) -> Result<Vec<u8>, HypervisorError> {
        let _ = self;
        let storage = crate::ceph::CephRbd;
        storage
            .create_snapshot(&req.pool_name, &req.volume_id, &req.snapshot_name)
            .await
            .map(|_| Self::ok_response("Snapshot created"))
            .map_err(|e| HypervisorError::ProcessError(e.to_string()))
    }

    async fn handle_delete_volume(
        &self,
        req: mikrom_proto::agent::DeleteVolumeRequest,
    ) -> Result<Vec<u8>, HypervisorError> {
        let _ = self;
        let storage = crate::ceph::CephRbd;
        storage
            .delete_volume(&req.pool_name, &req.volume_id)
            .await
            .map(|_| Self::ok_response("Volume deleted"))
            .map_err(|e| HypervisorError::ProcessError(e.to_string()))
    }

    async fn handle_delete_snapshot(
        &self,
        req: mikrom_proto::agent::DeleteSnapshotRequest,
    ) -> Result<Vec<u8>, HypervisorError> {
        let _ = self;
        let storage = crate::ceph::CephRbd;
        storage
            .delete_snapshot(&req.pool_name, &req.volume_id, &req.snapshot_name)
            .await
            .map(|_| Self::ok_response("Snapshot deleted"))
            .map_err(|e| HypervisorError::ProcessError(e.to_string()))
    }

    async fn handle_create_volume(
        &self,
        req: mikrom_proto::agent::CreateVolumeRequest,
    ) -> Result<Vec<u8>, HypervisorError> {
        let _ = self;
        let storage = crate::ceph::CephRbd;
        storage
            .create_volume(&req.pool_name, &req.volume_id, req.size_mib as i32)
            .await
            .map(|_| Self::ok_response("Volume created"))
            .map_err(|e| HypervisorError::ProcessError(e.to_string()))
    }

    async fn handle_restore_snapshot(
        &self,
        req: mikrom_proto::agent::RestoreSnapshotRequest,
    ) -> Result<Vec<u8>, HypervisorError> {
        let _ = self;
        let storage = crate::ceph::CephRbd;
        storage
            .restore_snapshot(&req.pool_name, &req.volume_id, &req.snapshot_name)
            .await
            .map(|_| Self::ok_response("Snapshot restored"))
            .map_err(|e| HypervisorError::ProcessError(e.to_string()))
    }

    async fn handle_clone_volume(
        &self,
        req: mikrom_proto::agent::CloneVolumeRequest,
    ) -> Result<Vec<u8>, HypervisorError> {
        let _ = self;
        let storage = crate::ceph::CephRbd;
        storage
            .clone_volume(
                &req.pool_name,
                &req.source_volume_id,
                &req.snapshot_name,
                &req.target_volume_id,
            )
            .await
            .map(|_| Self::ok_response("Volume cloned"))
            .map_err(|e| HypervisorError::ProcessError(e.to_string()))
    }

    // ── New VM runtime handlers ────────────────────────────────────────────

    async fn handle_vm_snapshot_create(
        &self,
        hv: &Arc<dyn VmHypervisor>,
        req: mikrom_proto::agent::VmSnapshotCreateRequest,
    ) -> Result<Vec<u8>, HypervisorError> {
        let vm_id = Self::parse_vm_id(&req.vm_id)?;
        hv.create_vm_snapshot(&vm_id, &req.snapshot_name)
            .await
            .map(|_| Self::ok_response("VM snapshot created"))
    }

    async fn handle_vm_snapshot_restore(
        &self,
        hv: &Arc<dyn VmHypervisor>,
        req: mikrom_proto::agent::VmSnapshotRestoreRequest,
    ) -> Result<Vec<u8>, HypervisorError> {
        let vm_id = Self::parse_vm_id(&req.vm_id)?;
        hv.restore_vm_snapshot(&vm_id, &req.snapshot_name)
            .await
            .map(|_| Self::ok_response("VM snapshot restored"))
    }

    async fn handle_vm_snapshot_delete(
        &self,
        hv: &Arc<dyn VmHypervisor>,
        req: mikrom_proto::agent::VmSnapshotDeleteRequest,
    ) -> Result<Vec<u8>, HypervisorError> {
        let vm_id = Self::parse_vm_id(&req.vm_id)?;
        hv.delete_vm_snapshot(&vm_id, &req.snapshot_name)
            .await
            .map(|_| Self::ok_response("VM snapshot deleted"))
    }

    async fn handle_vm_snapshot_list(
        &self,
        hv: &Arc<dyn VmHypervisor>,
        req: mikrom_proto::agent::VmSnapshotListRequest,
    ) -> Result<Vec<u8>, HypervisorError> {
        let vm_id = Self::parse_vm_id(&req.vm_id)?;
        let snapshots = hv.list_vm_snapshots(&vm_id).await?;
        let resp = mikrom_proto::agent::VmSnapshotListResponse {
            success: true,
            message: "OK".to_string(),
            snapshots,
        };
        let mut buf = Vec::new();
        resp.encode(&mut buf)
            .map_err(|e| HypervisorError::ProcessError(e.to_string()))?;
        Ok(buf)
    }

    async fn handle_attach_volume(
        &self,
        hv: &Arc<dyn VmHypervisor>,
        req: mikrom_proto::agent::AttachVolumeRequest,
    ) -> Result<Vec<u8>, HypervisorError> {
        let vm_id = Self::parse_vm_id(&req.vm_id)?;
        hv.attach_volume(&vm_id, &req.volume_id, &req.mount_point, req.read_only)
            .await
            .map(|_| Self::ok_response("Volume attached"))
    }

    async fn handle_detach_volume(
        &self,
        hv: &Arc<dyn VmHypervisor>,
        req: mikrom_proto::agent::DetachVolumeRequest,
    ) -> Result<Vec<u8>, HypervisorError> {
        let vm_id = Self::parse_vm_id(&req.vm_id)?;
        hv.detach_volume(&vm_id, &req.volume_id)
            .await
            .map(|_| Self::ok_response("Volume detached"))
    }

    async fn handle_start_migration(
        &self,
        hv: &Arc<dyn VmHypervisor>,
        req: mikrom_proto::agent::StartMigrationRequest,
    ) -> Result<Vec<u8>, HypervisorError> {
        let vm_id = Self::parse_vm_id(&req.vm_id)?;
        hv.start_migration(&vm_id, &req.target_host, &req.target_uri)
            .await
            .map(|_| Self::ok_response("Migration started"))
    }

    async fn handle_cancel_migration(
        &self,
        hv: &Arc<dyn VmHypervisor>,
        req: mikrom_proto::agent::CancelMigrationRequest,
    ) -> Result<Vec<u8>, HypervisorError> {
        let vm_id = Self::parse_vm_id(&req.vm_id)?;
        hv.cancel_migration(&vm_id)
            .await
            .map(|_| Self::ok_response("Migration cancelled"))
    }

    async fn handle_query_migration(
        &self,
        hv: &Arc<dyn VmHypervisor>,
        req: mikrom_proto::agent::QueryMigrationRequest,
    ) -> Result<Vec<u8>, HypervisorError> {
        let vm_id = Self::parse_vm_id(&req.vm_id)?;
        let status = hv.query_migration(&vm_id).await?;
        let resp = mikrom_proto::agent::QueryMigrationResponse {
            success: true,
            message: "OK".to_string(),
            status,
            total_bytes: 0,
            transferred_bytes: 0,
            remaining_bytes: 0,
        };
        let mut buf = Vec::new();
        resp.encode(&mut buf)
            .map_err(|e| HypervisorError::ProcessError(e.to_string()))?;
        Ok(buf)
    }

    async fn handle_set_balloon(
        &self,
        hv: &Arc<dyn VmHypervisor>,
        req: mikrom_proto::agent::SetBalloonRequest,
    ) -> Result<Vec<u8>, HypervisorError> {
        let vm_id = Self::parse_vm_id(&req.vm_id)?;
        hv.set_balloon_size(&vm_id, req.target_memory_mib)
            .await
            .map(|_| Self::ok_response("Balloon size set"))
    }

    async fn handle_query_balloon(
        &self,
        hv: &Arc<dyn VmHypervisor>,
        req: mikrom_proto::agent::QueryBalloonRequest,
    ) -> Result<Vec<u8>, HypervisorError> {
        let vm_id = Self::parse_vm_id(&req.vm_id)?;
        let (actual, max) = hv.query_balloon(&vm_id).await?;
        let resp = mikrom_proto::agent::QueryBalloonResponse {
            success: true,
            message: "OK".to_string(),
            actual_memory_mib: actual,
            max_memory_mib: max,
        };
        let mut buf = Vec::new();
        resp.encode(&mut buf)
            .map_err(|e| HypervisorError::ProcessError(e.to_string()))?;
        Ok(buf)
    }

    fn proto_vm_config(config: Option<mikrom_proto::agent::VmConfig>) -> VmConfig {
        let mut vm_config = VmConfig::default();
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
                .map(|v| Volume {
                    volume_id: v.volume_id,
                    size_mib: v.size_mib,
                    read_only: v.read_only,
                    pool_name: v.pool_name,
                    mount_point: v.mount_point,
                    access_mode: v.access_mode,
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

    fn parse_vm_id(vm_id: &str) -> Result<mikrom_proto::id::VmId, HypervisorError> {
        vm_id
            .parse::<mikrom_proto::id::VmId>()
            .map_err(|e| HypervisorError::ProcessError(format!("Invalid vm_id '{vm_id}': {e}")))
    }

    fn parse_app_id(app_id: &str) -> Result<mikrom_proto::id::AppId, HypervisorError> {
        app_id
            .parse::<mikrom_proto::id::AppId>()
            .map_err(|e| HypervisorError::ProcessError(format!("Invalid app_id '{app_id}': {e}")))
    }

    async fn reply_agent_command(
        message: async_nats::Message,
        nats: &async_nats::Client,
        result: Result<Vec<u8>, HypervisorError>,
    ) {
        if let Some(reply) = message.reply {
            match result {
                Ok(payload) => {
                    publish_best_effort(nats, reply.to_string(), payload, "agent-command-reply")
                        .await;
                },
                Err(e) => {
                    let response = mikrom_proto::agent::AgentCommandResponse {
                        success: false,
                        message: e.to_string(),
                    };
                    encode_and_publish_best_effort(nats, reply, &response, "agent-command-reply")
                        .await;
                },
            }
        }
    }

    /// Search all hypervisors for the VM and run health check.
    async fn handle_health_check(
        message: async_nats::Message,
        hypervisors: &HashMap<HypervisorType, Arc<dyn VmHypervisor>>,
        nats: &async_nats::Client,
        http_client: &reqwest::Client,
    ) {
        use mikrom_proto::agent::CheckHealthRequest;
        let Ok(req) = CheckHealthRequest::decode(&message.payload[..]) else {
            tracing::error!("Failed to decode CheckHealthRequest");
            return;
        };

        let vm_id: mikrom_proto::id::VmId = match req.vm_id.parse() {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(vm_id = %req.vm_id, error = %e, "Invalid VM ID in health check request");
                return Self::reply_health_check(message, nats, Err(format!("Invalid VM ID: {e}")))
                    .await;
            },
        };

        let result = match Self::find_vm_hypervisor(hypervisors, &vm_id).await {
            Some((vm, hv)) => {
                if matches!(vm.status, VmStatus::Failed) {
                    tracing::warn!(
                        vm_id = %vm_id,
                        error = vm.error_message.as_deref().unwrap_or("unknown error"),
                        "Skipping health check for failed VM"
                    );
                    return Self::reply_health_check(
                        message,
                        nats,
                        Err(vm
                            .error_message
                            .unwrap_or_else(|| "VM startup failed".to_string())),
                    )
                    .await;
                }

                let port = vm.config.port;
                let path = if vm.config.health_check_path.is_empty() {
                    "/".to_string()
                } else {
                    vm.config.health_check_path.clone()
                };
                let ip = vm.config.ipv6_address.clone();

                if let Some(ip_addr) = ip {
                    let started_at_ms = hv.get_vm_started_at_ms(&vm_id).await.unwrap_or_default();
                    let now_ms = chrono::Utc::now().timestamp_millis() as u64;
                    if started_at_ms > 0 && hv.is_app_started(&vm_id).await {
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

                    tracing::info!(
                        vm_id = %vm_id,
                        ip = %ip_addr,
                        port = port,
                        path = %path,
                        url = %Self::build_health_url(&ip_addr, port, &path),
                        "Performing health check..."
                    );

                    Self::perform_http_health_check(&ip_addr, port, &path, http_client).await
                } else {
                    Err("VM has no IPv6 address assigned (6PN required)".to_string())
                }
            },
            None => Err("VM not found".to_string()),
        };

        Self::reply_health_check(message, nats, result).await;
    }

    async fn find_vm_hypervisor(
        hypervisors: &HashMap<HypervisorType, Arc<dyn VmHypervisor>>,
        vm_id: &mikrom_proto::id::VmId,
    ) -> Option<(VmInfo, Arc<dyn VmHypervisor>)> {
        for hv in hypervisors.values() {
            if let Some(info) = hv.get_vm_info(vm_id).await {
                return Some((info, hv.clone()));
            }
        }
        None
    }

    fn build_health_url(ip: &str, port: u32, path: &str) -> String {
        if ip.contains(':') {
            format!("http://[{ip}]:{port}{path}")
        } else {
            format!("http://{ip}:{port}{path}")
        }
    }

    async fn perform_http_health_check(
        ip: &str,
        port: u32,
        path: &str,
        client: &reqwest::Client,
    ) -> Result<String, String> {
        let url = Self::build_health_url(ip, port, path);
        match tokio::time::timeout(Duration::from_secs(2), client.get(&url).send()).await {
            Ok(Ok(resp)) if resp.status().is_success() => Ok("Healthy".to_string()),
            Ok(Ok(resp)) => {
                let status = resp.status();
                tracing::warn!(
                    url = %url,
                    status = %status,
                    "Health check returned non-success status"
                );
                Err(format!("Unhealthy: HTTP {}", status))
            },
            Ok(Err(e)) => {
                tracing::warn!(
                    url = %url,
                    error = %e,
                    "Health check request failed"
                );
                Err(format!("Unhealthy: {e}"))
            },
            Err(_) => {
                tracing::warn!(
                    url = %url,
                    "Health check request timed out"
                );
                Err("Unhealthy: request timed out".to_string())
            },
        }
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
            encode_and_publish_best_effort(nats, reply, &response, "health-check-reply").await;
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
        let wireguard_port = i32::from(self.wg_manager.listen_port());
        let metrics_collector = self.metrics_collector.clone();
        let advertise_address = self.ip_address.clone();
        let supported_hypervisors: Vec<i32> = self.hypervisors.keys().map(|k| *k as i32).collect();

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
                                    VmStatus::Starting => ProtoVmStatus::Starting,
                                    VmStatus::Running => ProtoVmStatus::Running,
                                    VmStatus::Paused => ProtoVmStatus::Paused,
                                    VmStatus::Stopping => ProtoVmStatus::Stopping,
                                    VmStatus::Stopped => ProtoVmStatus::Stopped,
                                    VmStatus::Failed => ProtoVmStatus::Failed,
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
            config: self.config.clone(),
            ip_address: self.ip_address.clone(),
            metrics_collector: self.metrics_collector.clone(),
            hypervisors: self.hypervisors.clone(),
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
    use crate::hypervisor::{HypervisorType, VmHypervisor, VmInfo};
    use crate::qemu::{QemuConfig, QemuManager};
    use async_nats::Message as NatsMessage;
    use futures::StreamExt;
    use mikrom_proto::agent::{CheckHealthRequest, CheckHealthResponse};
    use mikrom_proto::id::{AppId, VmId};
    use prost::Message;
    use std::sync::Arc;
    use std::time::Duration;

    async fn make_hypervisors() -> HashMap<HypervisorType, Arc<dyn VmHypervisor>> {
        let mut hvs: HashMap<HypervisorType, Arc<dyn VmHypervisor>> = HashMap::new();
        hvs.insert(
            HypervisorType::Firecracker,
            Arc::new(FirecrackerManager::new().await),
        );
        hvs
    }

    #[tokio::test]
    async fn test_handle_health_check_vm_not_found() {
        let nats_url =
            std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
        let nats_client = async_nats::connect(nats_url).await.unwrap();
        let hypervisors = make_hypervisors().await;
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

        AgentServer::handle_health_check(
            message,
            &hypervisors,
            &nats_client,
            &reqwest::Client::new(),
        )
        .await;

        let resp_msg = tokio::time::timeout(std::time::Duration::from_secs(2), sub.next())
            .await
            .unwrap()
            .unwrap();
        let resp = CheckHealthResponse::decode(&resp_msg.payload[..]).unwrap();
        assert!(!resp.is_healthy);
        assert!(resp.message.contains("Invalid VM ID"));
    }

    #[tokio::test]
    async fn test_handle_health_check_skips_failed_vm() {
        let nats_url =
            std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
        let nats_client = async_nats::connect(nats_url).await.unwrap();
        let fc = FirecrackerManager::new().await;
        let mut hvs: HashMap<HypervisorType, Arc<dyn VmHypervisor>> = HashMap::new();
        hvs.insert(HypervisorType::Firecracker, Arc::new(fc.clone()));
        let reply = "test.reply.failed".to_string();
        let mut sub = nats_client.subscribe(reply.clone()).await.unwrap();

        let vm_id = VmId::new();
        {
            use crate::hypervisor::{VmConfig, VmInfo, VmStatus};
            let mut vms = fc.vms.write().await;
            vms.insert(
                vm_id,
                VmInfo {
                    vm_id,
                    app_id: AppId::new(),
                    image: "nginx".to_string(),
                    status: VmStatus::Failed,
                    config: VmConfig {
                        port: 8080,
                        health_check_path: "/".to_string(),
                        ipv6_address: Some("fd40:b90d:fc5f:1ae0::1".to_string()),
                        ..Default::default()
                    },
                    started_at: None,
                    error_message: Some("build failed".to_string()),
                },
            );
        }

        let req = CheckHealthRequest {
            vm_id: vm_id.to_string(),
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

        AgentServer::handle_health_check(message, &hvs, &nats_client, &reqwest::Client::new())
            .await;

        let resp_msg = tokio::time::timeout(std::time::Duration::from_secs(2), sub.next())
            .await
            .unwrap()
            .unwrap();
        let resp = CheckHealthResponse::decode(&resp_msg.payload[..]).unwrap();
        assert!(!resp.is_healthy);
        assert!(resp.message.contains("build failed"));
    }

    #[tokio::test]
    async fn test_handle_health_check_respects_boot_grace_window() {
        let nats_url =
            std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
        let nats_client = async_nats::connect(nats_url).await.unwrap();
        let fc = FirecrackerManager::new().await;
        let mut hvs: HashMap<HypervisorType, Arc<dyn VmHypervisor>> = HashMap::new();
        hvs.insert(HypervisorType::Firecracker, Arc::new(fc.clone()));
        let reply = "test.reply.booting".to_string();
        let mut sub = nats_client.subscribe(reply.clone()).await.unwrap();

        let vm_id = VmId::new();
        {
            use crate::hypervisor::{VmConfig, VmInfo, VmStatus};
            let mut vms = fc.vms.write().await;
            vms.insert(
                vm_id,
                VmInfo {
                    vm_id,
                    app_id: AppId::new(),
                    image: "nginx".to_string(),
                    status: VmStatus::Running,
                    config: VmConfig {
                        port: 8080,
                        health_check_path: "/".to_string(),
                        ipv6_address: Some("fd40:b90d:fc5f:1ae0::1".to_string()),
                        ..Default::default()
                    },
                    started_at: None,
                    error_message: None,
                },
            );
            let started_at_ms = chrono::Utc::now().timestamp_millis() as u64;
            fc.seed_started_process_for_test(vm_id, started_at_ms).await;
        }

        let req = CheckHealthRequest {
            vm_id: vm_id.to_string(),
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

        AgentServer::handle_health_check(message, &hvs, &nats_client, &reqwest::Client::new())
            .await;

        let resp_msg = tokio::time::timeout(std::time::Duration::from_secs(2), sub.next())
            .await
            .unwrap()
            .unwrap();
        let resp = CheckHealthResponse::decode(&resp_msg.payload[..]).unwrap();
        assert!(!resp.is_healthy);
        assert!(resp.message.contains("booting"));
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
        let mut hvs: HashMap<HypervisorType, Arc<dyn VmHypervisor>> = HashMap::new();
        hvs.insert(HypervisorType::Firecracker, Arc::new(fc.clone()));
        let reply = "test.reply.http".to_string();
        let mut sub = nats_client.subscribe(reply.clone()).await.unwrap();

        // 3. Register a fake VM in the manager so get_vm_info returns it
        let vm_id = VmId::new();
        {
            use crate::hypervisor::{VmConfig, VmInfo, VmStatus};
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

        AgentServer::handle_health_check(message, &hvs, &nats_client, &reqwest::Client::new())
            .await;

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

        AgentServer::handle_health_check(message2, &hvs, &nats_client, &reqwest::Client::new())
            .await;
        let resp_msg = tokio::time::timeout(std::time::Duration::from_secs(2), sub.next())
            .await
            .unwrap()
            .unwrap();
        let resp = CheckHealthResponse::decode(&resp_msg.payload[..]).unwrap();
        assert!(!resp.is_healthy, "Should be unhealthy for 302 Redirect");
        assert!(resp.message.contains("HTTP 302 Found"));
    }

    #[test]
    fn test_is_flapping_nats_session_flags_short_sessions_only() {
        assert!(super::is_flapping_nats_session(
            Some(Duration::from_secs(1)),
            30
        ));
        assert!(super::is_flapping_nats_session(
            Some(Duration::from_secs(29)),
            30
        ));
        assert!(!super::is_flapping_nats_session(
            Some(Duration::from_secs(30)),
            30
        ));
        assert!(!super::is_flapping_nats_session(
            Some(Duration::from_secs(120)),
            30
        ));
        assert!(!super::is_flapping_nats_session(None, 30));
    }

    // ── Hypervisor routing tests ─────────────────────────────────

    #[tokio::test]
    async fn test_resolve_hypervisor_for_start_selects_firecracker() {
        let hvs = make_hypervisors().await;
        let req = mikrom_proto::agent::StartVmRequest {
            vm_id: "vm-1".into(),
            app_id: "app-1".into(),
            image: "img".into(),
            config: Some(mikrom_proto::agent::VmConfig {
                hypervisor: 1,
                ..Default::default()
            }),
        };
        let hv = AgentServer::resolve_hypervisor_for_start(&req, &hvs);
        assert!(hv.is_some());
        assert_eq!(hv.unwrap().hypervisor_type(), HypervisorType::Firecracker);
    }

    #[tokio::test]
    async fn test_resolve_hypervisor_for_start_selects_qemu() {
        let fc = FirecrackerManager::new().await;
        let qemu = QemuManager::new("test-agent".into()).await;
        let mut hvs: HashMap<HypervisorType, Arc<dyn VmHypervisor>> = HashMap::new();
        hvs.insert(HypervisorType::Firecracker, Arc::new(fc));
        hvs.insert(HypervisorType::QemuMicrovm, Arc::new(qemu));
        let req = mikrom_proto::agent::StartVmRequest {
            vm_id: "vm-1".into(),
            app_id: "app-1".into(),
            image: "img".into(),
            config: Some(mikrom_proto::agent::VmConfig {
                hypervisor: 2,
                ..Default::default()
            }),
        };
        let hv = AgentServer::resolve_hypervisor_for_start(&req, &hvs);
        assert!(hv.is_some());
        assert_eq!(hv.unwrap().hypervisor_type(), HypervisorType::QemuMicrovm);
    }

    #[tokio::test]
    async fn test_resolve_hypervisor_for_start_defaults_to_firecracker() {
        let hvs = make_hypervisors().await;
        let req = mikrom_proto::agent::StartVmRequest {
            vm_id: "vm-1".into(),
            app_id: "app-1".into(),
            image: "img".into(),
            config: Some(mikrom_proto::agent::VmConfig {
                hypervisor: 0,
                ..Default::default()
            }),
        };
        let hv = AgentServer::resolve_hypervisor_for_start(&req, &hvs);
        assert!(hv.is_some());
        assert_eq!(hv.unwrap().hypervisor_type(), HypervisorType::Firecracker);
    }

    #[tokio::test]
    async fn test_resolve_hypervisor_for_start_no_config() {
        let hvs = make_hypervisors().await;
        let req = mikrom_proto::agent::StartVmRequest {
            vm_id: "vm-1".into(),
            app_id: "app-1".into(),
            image: "img".into(),
            config: None,
        };
        let hv = AgentServer::resolve_hypervisor_for_start(&req, &hvs);
        assert!(hv.is_none());
    }

    #[tokio::test]
    async fn test_find_hypervisor_for_vm_finds_on_firecracker() {
        let fc = FirecrackerManager::new().await;
        let qemu = QemuManager::new("test-agent".into()).await;
        let mut hvs: HashMap<HypervisorType, Arc<dyn VmHypervisor>> = HashMap::new();
        hvs.insert(HypervisorType::Firecracker, Arc::new(fc.clone()));
        hvs.insert(HypervisorType::QemuMicrovm, Arc::new(qemu));

        let vm_id = VmId::new();
        let vm_id_copy = vm_id;
        // Insert directly into internal state to avoid Firecracker binary check
        fc.vms.write().await.insert(
            vm_id_copy,
            VmInfo {
                vm_id,
                app_id: AppId::new(),
                image: "img".into(),
                config: VmConfig::default(),
                status: VmStatus::Running,
                started_at: None,
                error_message: None,
            },
        );

        let found = AgentServer::find_hypervisor_for_vm(&vm_id, &hvs).await;
        assert!(found.is_some());
        assert_eq!(
            found.unwrap().hypervisor_type(),
            HypervisorType::Firecracker
        );
    }

    #[tokio::test]
    async fn test_find_hypervisor_for_vm_finds_on_qemu() {
        let fc = FirecrackerManager::new().await;
        let qemu = QemuManager::with_config(
            "test-agent".into(),
            QemuConfig {
                binary: "/bin/sleep".into(),
                kernel_path: "/dev/null".into(),
                rootfs_path: "/dev/null".into(),
                base_rootfs_path: "/dev/null".into(),
                data_dir: std::env::temp_dir().join("qemu-test"),
                qmp_timeout_secs: 1,
                extra_args: vec!["3600".into()],
                kernel_url: None,
                rootfs_url: None,
                image_cache_dir: std::env::temp_dir().join("qemu-image-cache-test"),
                virtiofsd_binary: String::new(),
                virtiofsd_socket_dir: std::env::temp_dir().join("qemu-virtiofsd-test"),
                virtiofsd_shares: Vec::new(),
            },
        )
        .await;
        let mut hvs: HashMap<HypervisorType, Arc<dyn VmHypervisor>> = HashMap::new();
        hvs.insert(HypervisorType::Firecracker, Arc::new(fc));
        hvs.insert(HypervisorType::QemuMicrovm, Arc::new(qemu.clone()));

        let vm_id = VmId::new();
        let app_id = AppId::new();
        qemu.start_vm(vm_id, app_id, "img".into(), VmConfig::default())
            .await
            .unwrap();

        let found = AgentServer::find_hypervisor_for_vm(&vm_id, &hvs).await;
        assert!(found.is_some());
        assert_eq!(
            found.unwrap().hypervisor_type(),
            HypervisorType::QemuMicrovm
        );

        // Cleanup
        let _ = qemu.stop_vm(&vm_id).await;
    }

    #[tokio::test]
    async fn test_find_hypervisor_for_vm_not_found() {
        let fc = FirecrackerManager::new().await;
        let qemu = QemuManager::with_config(
            "test-agent".into(),
            QemuConfig {
                binary: "/bin/sleep".into(),
                kernel_path: "/dev/null".into(),
                rootfs_path: "/dev/null".into(),
                base_rootfs_path: "/dev/null".into(),
                data_dir: std::env::temp_dir().join("qemu-test"),
                qmp_timeout_secs: 1,
                extra_args: vec!["3600".into()],
                kernel_url: None,
                rootfs_url: None,
                image_cache_dir: std::env::temp_dir().join("qemu-image-cache-test"),
                virtiofsd_binary: String::new(),
                virtiofsd_socket_dir: std::env::temp_dir().join("qemu-virtiofsd-test"),
                virtiofsd_shares: Vec::new(),
            },
        )
        .await;
        let mut hvs: HashMap<HypervisorType, Arc<dyn VmHypervisor>> = HashMap::new();
        hvs.insert(HypervisorType::Firecracker, Arc::new(fc));
        hvs.insert(HypervisorType::QemuMicrovm, Arc::new(qemu));

        let ghost = VmId::new();
        let found = AgentServer::find_hypervisor_for_vm(&ghost, &hvs).await;
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_extract_vm_id_from_start_vm() {
        use mikrom_proto::agent::agent_command::Command;
        let cmd = Command::StartVm(mikrom_proto::agent::StartVmRequest {
            vm_id: "test-vm".into(),
            app_id: "app-1".into(),
            image: "img".into(),
            config: None,
        });
        assert_eq!(
            AgentServer::extract_vm_id_from_command(&cmd),
            Some("test-vm")
        );
    }

    #[tokio::test]
    async fn test_extract_vm_id_from_stop_vm() {
        use mikrom_proto::agent::agent_command::Command;
        let cmd = Command::StopVm(mikrom_proto::agent::StopVmRequest {
            vm_id: "stop-vm".into(),
        });
        assert_eq!(
            AgentServer::extract_vm_id_from_command(&cmd),
            Some("stop-vm")
        );
    }

    #[tokio::test]
    async fn test_extract_vm_id_from_snapshot_returns_none() {
        use mikrom_proto::agent::agent_command::Command;
        let cmd = Command::CreateSnapshot(mikrom_proto::agent::CreateSnapshotRequest {
            volume_id: "vol-1".into(),
            snapshot_name: "snap-1".into(),
            pool_name: "rbd".into(),
        });
        assert!(AgentServer::extract_vm_id_from_command(&cmd).is_none());
    }
}
