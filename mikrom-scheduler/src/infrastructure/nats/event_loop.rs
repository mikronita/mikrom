use crate::domain::{HostMetrics, Worker};
use crate::server::SchedulerServer;
use futures::StreamExt;
use mikrom_proto::scheduler::{
    AppStatusRequest, CancelRequest, DeleteAllByAppRequest, DeleteAppRequest, DeployRequest,
    ListAppsRequest, ListWorkersRequest, PauseRequest, ResumeRequest, RouterHeartbeat,
    WorkerHeartbeat,
};
use prost::Message;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

const MESH_PRUNING_THRESHOLD_SECS: i64 = 30;
const DEFAULT_RESTORE_RETRY_BACKOFF_SECS: i64 = 3600;

pub struct NatsEventLoop {
    server: SchedulerServer,
    client: async_nats::Client,
    queue_group: String,
    router_restore_in_progress: Arc<Mutex<HashSet<String>>>,
}

impl NatsEventLoop {
    pub fn new(server: SchedulerServer, client: async_nats::Client) -> Self {
        Self {
            server,
            client,
            queue_group: "schedulers".to_string(),
            router_restore_in_progress: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn with_queue_group(mut self, group: String) -> Self {
        self.queue_group = group;
        self
    }

    fn restore_retry_backoff_secs_from(value: Option<&str>) -> i64 {
        value
            .and_then(|raw| raw.parse::<i64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_RESTORE_RETRY_BACKOFF_SECS)
    }

    fn restore_retry_backoff_secs() -> i64 {
        let env_value = std::env::var("MIKROM_RESTORE_RETRY_BACKOFF_SECS").ok();
        Self::restore_retry_backoff_secs_from(env_value.as_deref())
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let client = self.client.clone();
        let queue_group = self.queue_group.clone();

        // 1. Heartbeats
        let mut heartbeat_sub = client
            .subscribe("mikrom.scheduler.worker.heartbeat")
            .await?;

        let mut router_heartbeat_sub = client
            .subscribe("mikrom.scheduler.router.heartbeat")
            .await?;
        let mut router_traffic_sub = client
            .queue_subscribe(
                mikrom_proto::subjects::ROUTER_TRAFFIC_EVENT,
                queue_group.clone(),
            )
            .await?;

        // 2. Queue Group Subscriptions for Load Balancing
        let mut deploy_sub = client
            .queue_subscribe("mikrom.scheduler.deploy", queue_group.clone())
            .await?;
        let mut status_sub = client
            .queue_subscribe("mikrom.scheduler.get_job", queue_group.clone())
            .await?;
        let mut list_sub = client
            .queue_subscribe("mikrom.scheduler.list_apps", queue_group.clone())
            .await?;
        let mut list_workers_sub = client
            .queue_subscribe("mikrom.scheduler.list_workers", queue_group.clone())
            .await?;
        let mut pause_sub = client
            .queue_subscribe("mikrom.scheduler.pause_app", queue_group.clone())
            .await?;
        let mut resume_sub = client
            .queue_subscribe("mikrom.scheduler.resume_app", queue_group.clone())
            .await?;
        let mut cancel_sub = client
            .queue_subscribe("mikrom.scheduler.cancel_app", queue_group.clone())
            .await?;
        let mut delete_sub = client
            .queue_subscribe("mikrom.scheduler.delete_app", queue_group.clone())
            .await?;
        let mut delete_all_sub = client
            .queue_subscribe("mikrom.scheduler.delete_all_by_app", queue_group.clone())
            .await?;
        let mut scale_sub = client
            .queue_subscribe(
                mikrom_proto::subjects::SCHEDULER_SCALE_APP,
                queue_group.clone(),
            )
            .await?;
        let mut health_sub = client
            .queue_subscribe("mikrom.scheduler.check_health", queue_group.clone())
            .await?;
        let mut security_sub = client
            .queue_subscribe(
                "mikrom.scheduler.update_security_groups",
                queue_group.clone(),
            )
            .await?;
        let mut vm_failed_sub = client
            .queue_subscribe(
                mikrom_proto::subjects::SCHEDULER_VM_FAILED,
                queue_group.clone(),
            )
            .await?;
        let mut update_scaling_sub = client
            .queue_subscribe(
                mikrom_proto::subjects::SCHEDULER_UPDATE_APP_SCALING_CONFIG,
                queue_group.clone(),
            )
            .await?;
        let mut create_volume_sub = client
            .queue_subscribe("mikrom.scheduler.create_volume", queue_group.clone())
            .await?;
        let mut snapshot_sub = client
            .queue_subscribe("mikrom.scheduler.create_snapshot", queue_group.clone())
            .await?;
        let mut delete_volume_sub = client
            .queue_subscribe("mikrom.scheduler.delete_volume", queue_group.clone())
            .await?;
        let mut delete_snapshot_sub = client
            .queue_subscribe("mikrom.scheduler.delete_snapshot", queue_group.clone())
            .await?;
        let mut restore_snapshot_sub = client
            .queue_subscribe("mikrom.scheduler.restore_snapshot", queue_group.clone())
            .await?;
        let mut clone_volume_sub = client
            .queue_subscribe("mikrom.scheduler.clone_volume", queue_group.clone())
            .await?;

        tracing::info!("NATS Event Loop started, listening for messages...");

        let mut mesh_interval = tokio::time::interval(tokio::time::Duration::from_secs(1));

        loop {
            tokio::select! {
                _ = mesh_interval.tick() => {
                    Self::broadcast_mesh_updates(self.server.clone(), self.client.clone()).await;
                }
                Some(msg) = heartbeat_sub.next() => self.handle_heartbeat(msg).await,
                Some(msg) = router_heartbeat_sub.next() => self.handle_router_heartbeat(msg).await,
                Some(msg) = router_traffic_sub.next() => self.handle_router_traffic(msg).await,
                Some(msg) = deploy_sub.next() => self.handle_deploy(msg).await,
                Some(msg) = status_sub.next() => self.handle_status(msg).await,
                Some(msg) = list_sub.next() => self.handle_list_apps(msg).await,
                Some(msg) = list_workers_sub.next() => self.handle_list_workers(msg).await,
                Some(msg) = pause_sub.next() => self.handle_pause(msg).await,
                Some(msg) = resume_sub.next() => self.handle_resume(msg).await,
                Some(msg) = cancel_sub.next() => self.handle_cancel(msg).await,
                Some(msg) = delete_sub.next() => self.handle_delete(msg).await,
                Some(msg) = delete_all_sub.next() => self.handle_delete_all(msg).await,
                Some(msg) = scale_sub.next() => self.handle_scale(msg).await,
                Some(msg) = health_sub.next() => self.handle_check_health(msg).await,
                Some(msg) = security_sub.next() => self.handle_update_security_groups(msg).await,
                Some(msg) = vm_failed_sub.next() => self.handle_vm_failed(msg).await,
                Some(msg) = update_scaling_sub.next() => self.handle_update_app_scaling_config(msg).await,
                Some(msg) = create_volume_sub.next() => self.handle_create_volume(msg).await,
                Some(msg) = snapshot_sub.next() => self.handle_create_snapshot(msg).await,

                Some(msg) = delete_volume_sub.next() => self.handle_delete_volume(msg).await,
                Some(msg) = delete_snapshot_sub.next() => self.handle_delete_snapshot(msg).await,
                Some(msg) = restore_snapshot_sub.next() => self.handle_restore_snapshot(msg).await,
                Some(msg) = clone_volume_sub.next() => self.handle_clone_volume(msg).await,
            }
        }
    }

    fn acquire_router_restore_guard(
        router_restore_in_progress: &Arc<Mutex<HashSet<String>>>,
        app_id: &str,
    ) -> Option<RouterRestoreGuard> {
        let mut in_progress = router_restore_in_progress
            .lock()
            .expect("router restore guard mutex poisoned");

        if !in_progress.insert(app_id.to_string()) {
            tracing::debug!(event = "router_restore_deduplicated", app_id = %app_id, "Router restore already in progress");
            return None;
        }

        Some(RouterRestoreGuard {
            router_restore_in_progress: router_restore_in_progress.clone(),
            app_id: app_id.to_string(),
        })
    }

    async fn broadcast_mesh_updates(server: SchedulerServer, client: async_nats::Client) {
        let updates = Self::calculate_mesh_updates(&server).await;

        for (host_id, update) in updates {
            let mut buf = Vec::new();
            if update.encode(&mut buf).is_ok() {
                let subject = format!("mikrom.scheduler.network.mesh.{}", host_id);
                let _ = client.publish(subject, buf.into()).await;
            }
        }
    }

    async fn calculate_mesh_updates(
        server: &SchedulerServer,
    ) -> std::collections::HashMap<String, mikrom_proto::scheduler::NetworkMeshUpdate> {
        let mut updates = std::collections::HashMap::new();

        let Ok(workers) = server.app_service.worker_repo.list_workers().await else {
            return updates;
        };

        // Filter for active workers only
        let now = chrono::Utc::now().timestamp();
        let active_workers: Vec<_> = workers
            .iter()
            .filter(|w| now - w.last_heartbeat < MESH_PRUNING_THRESHOLD_SECS)
            .collect();

        // 1. Fetch all active jobs grouped by host_id
        let mut jobs_by_host = std::collections::HashMap::new();
        if let Ok(jobs) = server
            .app_service
            .job_repo
            .list_jobs(None, None, None)
            .await
        {
            for job in jobs {
                if !matches!(
                    job.status,
                    crate::domain::JobStatus::Pending
                        | crate::domain::JobStatus::Scheduled
                        | crate::domain::JobStatus::Running
                        | crate::domain::JobStatus::Paused
                ) {
                    continue;
                }

                if let Some(host_id) = &job.host_id {
                    jobs_by_host
                        .entry(host_id.clone())
                        .or_insert_with(Vec::new)
                        .push(job);
                }
            }
        }

        // 2. Pre-build ALL potential peers
        let all_peers: Vec<mikrom_proto::scheduler::Peer> = active_workers
            .iter()
            .filter(|w| w.wireguard_pubkey.is_some())
            .map(|w| {
                let mut allowed_ips = Vec::new();
                if let Some(wg_ip) = &w.wireguard_ip
                    && !wg_ip.is_empty()
                {
                    let prefix = if wg_ip.contains(':') { "/128" } else { "/32" };
                    allowed_ips.push(format!("{}{}", wg_ip, prefix));
                }

                if let Some(jobs) = jobs_by_host.get(&w.host_id) {
                    for job in jobs {
                        if let Some(ipv6) = &job.config.ipv6_address {
                            let prefix = if ipv6.contains(':') { "/128" } else { "/32" };
                            allowed_ips.push(format!("{}{}", ipv6, prefix));
                        }
                    }
                }

                // Sort allowed IPs for idempotency
                allowed_ips.sort();

                mikrom_proto::scheduler::Peer {
                    host_id: w.host_id.clone(),
                    endpoint: w.advertise_address.clone(),
                    wireguard_pubkey: w.wireguard_pubkey.clone().unwrap_or_default(),
                    allowed_ips,
                    wireguard_port: w.wireguard_port.unwrap_or(51820),
                }
            })
            .collect();

        // 3. Build update for each worker (even if inactive, to tell them they are alone if they wake up)
        for w in &workers {
            let peers: Vec<_> = all_peers
                .iter()
                .filter(|p| p.host_id != w.host_id)
                .cloned()
                .collect();

            updates.insert(
                w.host_id.clone(),
                mikrom_proto::scheduler::NetworkMeshUpdate { peers },
            );
        }

        updates
    }

    async fn handle_update_security_groups(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            if let Ok(req) =
                mikrom_proto::scheduler::UpdateSecurityGroupsRequest::decode(&message.payload[..])
            {
                let result = server.update_security_groups(req).await;
                if let Some(reply) = message.reply {
                    let response = match result {
                        Ok(resp) => resp,
                        Err(e) => mikrom_proto::scheduler::UpdateSecurityGroupsResponse {
                            success: false,
                            message: e.to_string(),
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

    async fn handle_update_app_scaling_config(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            use mikrom_proto::scheduler::UpdateAppScalingConfigRequest;
            if let Ok(req) = UpdateAppScalingConfigRequest::decode(&message.payload[..]) {
                let result = server.update_app_scaling_config(req).await;
                if let Some(reply) = message.reply {
                    let response = match result {
                        Ok(resp) => resp,
                        Err(e) => mikrom_proto::scheduler::UpdateAppScalingConfigResponse {
                            success: false,
                            message: e.to_string(),
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

    async fn handle_check_health(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            if let Ok(req) =
                mikrom_proto::scheduler::CheckHealthRequest::decode(&message.payload[..])
            {
                tracing::info!(
                    job_id = %req.job_id,
                    user_id = %req.user_id,
                    "Received scheduler check-health request"
                );
                let result = server.check_health(req).await;
                if let Some(reply) = message.reply {
                    let response = match result {
                        Ok(resp) => {
                            tracing::info!(
                                is_healthy = resp.is_healthy,
                                "Scheduler check-health request completed"
                            );
                            resp
                        },
                        Err(e) => mikrom_proto::scheduler::CheckHealthResponse {
                            is_healthy: false,
                            message: e.to_string(),
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

    async fn handle_router_heartbeat(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            match RouterHeartbeat::decode(&message.payload[..]) {
                Ok(heartbeat) => {
                    tracing::debug!(
                        "Received heartbeat from router {} with WG IP {}",
                        heartbeat.host_id,
                        heartbeat.wireguard_ip
                    );
                    let worker = Worker {
                        host_id: heartbeat.host_id.clone(),
                        hostname: heartbeat.hostname.clone(),
                        advertise_address: heartbeat.advertise_address.clone(),
                        wireguard_pubkey: Some(heartbeat.wireguard_pubkey.clone()),
                        wireguard_ip: Some(heartbeat.wireguard_ip.clone()),
                        wireguard_port: Some(heartbeat.wireguard_port),
                        metrics: None,
                        registered_at: chrono::Utc::now().timestamp(),
                        last_heartbeat: chrono::Utc::now().timestamp(),
                    };

                    if let Err(e) = server.app_service.worker_repo.register(worker).await {
                        tracing::error!(
                            "Failed to register router {} from heartbeat: {}",
                            heartbeat.host_id,
                            e
                        );
                    } else {
                        // Fast-path: Broadcast mesh update immediately on new registration
                        Self::broadcast_mesh_updates(server, client).await;
                    }
                },
                Err(e) => {
                    tracing::error!("Failed to decode router heartbeat: {}", e);
                },
            }
        });
    }

    async fn handle_router_traffic(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let router_restore_in_progress = self.router_restore_in_progress.clone();
        tokio::spawn(async move {
            match mikrom_proto::router::RouterTrafficEvent::decode(&message.payload[..]) {
                Ok(event) => {
                    tracing::info!(
                        hostname = %event.hostname,
                        router_id = %event.router_id,
                        timestamp = %event.timestamp,
                        "Received router traffic event"
                    );

                    let Some(mut app) = server
                        .app_service
                        .app_repo
                        .get_app_config_by_hostname(&event.hostname)
                        .await
                        .ok()
                        .flatten()
                    else {
                        return;
                    };

                    let timestamp = if event.timestamp > 0 {
                        event.timestamp
                    } else {
                        chrono::Utc::now().timestamp()
                    };

                    app.last_router_traffic_at = timestamp;
                    if let Err(e) = server
                        .app_service
                        .app_repo
                        .update_app_config(app.clone())
                        .await
                    {
                        tracing::error!(
                            hostname = %event.hostname,
                            error = %e,
                            "Failed to persist router traffic timestamp"
                        );
                        return;
                    }

                    let restore_retry_blocked =
                        app.restore_retry_after_at > 0 && timestamp < app.restore_retry_after_at;

                    let current_count = server
                        .app_service
                        .job_repo
                        .list_jobs(Some(&app.user_id), Some(&app.id), None)
                        .await
                        .map(|jobs| {
                            jobs.into_iter()
                                .filter(|job| {
                                    matches!(
                                        job.status,
                                        crate::domain::JobStatus::Pending
                                            | crate::domain::JobStatus::Scheduled
                                            | crate::domain::JobStatus::Running
                                    )
                                })
                                .count() as u32
                        })
                        .unwrap_or_default();

                    if current_count == 0 && app.desired_replicas > 0 {
                        if restore_retry_blocked {
                            tracing::warn!(
                                app_id = %app.id,
                                hostname = %event.hostname,
                                retry_after = %app.restore_retry_after_at,
                                "Skipping router-triggered restore while backoff is active"
                            );
                            return;
                        }

                        let Some(_restore_guard) = Self::acquire_router_restore_guard(
                            &router_restore_in_progress,
                            &app.id,
                        ) else {
                            return;
                        };

                        tracing::info!(
                            event = "restore_from_router_traffic",
                            app_id = %app.id,
                            hostname = %event.hostname,
                            desired = %app.desired_replicas,
                            "Router traffic arrived for a scaled-to-zero app; restoring replicas"
                        );

                        if let Err(e) = server
                            .app_service
                            .scale_app(&app.id, app.desired_replicas, &app.user_id)
                            .await
                        {
                            tracing::error!(
                                app_id = %app.id,
                                hostname = %event.hostname,
                                error = %e,
                                "Failed to restore app after router traffic"
                            );
                        } else {
                            let _ = server
                                .app_service
                                .app_repo
                                .update_app_config(crate::domain::AppConfig {
                                    last_scaled_to_zero_at: timestamp,
                                    restore_retry_after_at: 0,
                                    ..app.clone()
                                })
                                .await;
                            tracing::info!(
                                event = "restore_from_router_traffic_completed",
                                app_id = %app.id,
                                hostname = %event.hostname,
                                desired = %app.desired_replicas,
                                "App restored after router traffic"
                            );
                        }
                    }
                },
                Err(e) => {
                    tracing::error!("Failed to decode router traffic event: {}", e);
                },
            }
        });
    }

    async fn handle_heartbeat(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            match WorkerHeartbeat::decode(&message.payload[..]) {
                Ok(heartbeat) => {
                    use mikrom_proto::scheduler::VmStatus as ProtoVmStatus;

                    tracing::debug!(
                        "Received heartbeat from worker {} with WG IP {}",
                        heartbeat.host_id,
                        heartbeat.wireguard_ip
                    );
                    let worker = Worker {
                        host_id: heartbeat.host_id.clone(),
                        hostname: heartbeat.hostname.clone(),
                        advertise_address: heartbeat.advertise_address.clone(),
                        wireguard_pubkey: Some(heartbeat.wireguard_pubkey.clone()),
                        wireguard_ip: Some(heartbeat.wireguard_ip.clone()),
                        wireguard_port: Some(heartbeat.wireguard_port),
                        metrics: None, // Will update below
                        registered_at: chrono::Utc::now().timestamp(),
                        last_heartbeat: chrono::Utc::now().timestamp(),
                    };

                    if let Err(e) = server.app_service.worker_repo.register(worker).await {
                        tracing::error!(
                            "Failed to register worker {} from heartbeat: {}",
                            heartbeat.host_id,
                            e
                        );
                    } else {
                        // Fast-path: Broadcast mesh update immediately on new registration
                        Self::broadcast_mesh_updates(server.clone(), client.clone()).await;
                    }

                    if let Some(metrics) = heartbeat.metrics {
                        let running_jobs_by_vm = server
                            .app_service
                            .job_repo
                            .list_jobs(None, None, Some(crate::domain::JobStatus::Running))
                            .await
                            .map(|jobs| {
                                jobs.into_iter()
                                    .filter(|job| {
                                        job.host_id.as_deref() == Some(&heartbeat.host_id)
                                    })
                                    .filter_map(|job| job.vm_id.clone().map(|vm_id| (vm_id, job)))
                                    .collect::<std::collections::HashMap<_, _>>()
                            })
                            .unwrap_or_default();

                        for (vm_id, vm_metrics) in &metrics.vms {
                            if vm_metrics.status != ProtoVmStatus::Failed as i32 {
                                continue;
                            }

                            let Some(job) = running_jobs_by_vm.get(vm_id) else {
                                continue;
                            };

                            let message = if vm_metrics.error_message.is_empty() {
                                "VM startup failed".to_string()
                            } else {
                                vm_metrics.error_message.clone()
                            };

                            tracing::error!(
                                job_id = %job.job_id,
                                vm_id = %vm_id,
                                host_id = %heartbeat.host_id,
                                error = %message,
                                "Detected failed VM in worker heartbeat"
                            );

                            if let Err(e) = server
                                .app_service
                                .job_repo
                                .fail_job(
                                    &job.job_id,
                                    message.clone(),
                                    chrono::Utc::now().timestamp(),
                                )
                                .await
                            {
                                tracing::error!(
                                    job_id = %job.job_id,
                                    vm_id = %vm_id,
                                    error = %e,
                                    "Failed to persist failed VM state"
                                );
                                continue;
                            }

                            let mut updated_job = job.clone();
                            updated_job.status = crate::domain::JobStatus::Failed;
                            updated_job.stopped_at = Some(chrono::Utc::now().timestamp());
                            updated_job.error_message = Some(message);

                            if let Ok(Some(mut app)) = server
                                .app_service
                                .app_repo
                                .get_app_config(&updated_job.app_id)
                                .await
                            {
                                let retry_after = chrono::Utc::now().timestamp()
                                    + Self::restore_retry_backoff_secs();
                                app.restore_retry_after_at = retry_after;
                                if let Err(e) =
                                    server.app_service.app_repo.update_app_config(app).await
                                {
                                    tracing::error!(
                                        app_id = %updated_job.app_id,
                                        retry_after = %retry_after,
                                        error = %e,
                                        "Failed to persist restore backoff after failed VM heartbeat"
                                    );
                                }
                            }

                            use mikrom_proto::scheduler::AppInfo;
                            use prost::Message;

                            let info = AppInfo {
                                job_id: updated_job.job_id.clone(),
                                app_id: updated_job.app_id.clone(),
                                app_name: updated_job.app_name.clone(),
                                image: updated_job.image.clone(),
                                status: updated_job.status as i32,
                                host_id: updated_job.host_id.clone().unwrap_or_default(),
                                vm_id: updated_job.vm_id.clone().unwrap_or_default(),
                                user_id: updated_job.user_id.clone(),
                                deployment_id: updated_job
                                    .deployment_id
                                    .clone()
                                    .unwrap_or_default(),
                                ipv6_address: updated_job
                                    .config
                                    .ipv6_address
                                    .clone()
                                    .unwrap_or_default(),
                                ..Default::default()
                            };

                            let mut buf = Vec::new();
                            if info.encode(&mut buf).is_ok() {
                                let _ = client
                                    .publish(
                                        mikrom_proto::subjects::SCHEDULER_JOB_UPDATES,
                                        buf.into(),
                                    )
                                    .await;
                            }
                        }

                        for (vm_id, vm_metrics) in &metrics.vms {
                            if vm_metrics.status == ProtoVmStatus::Failed as i32 {
                                continue;
                            }

                            let Some(job) = running_jobs_by_vm.get(vm_id) else {
                                continue;
                            };

                            let event = serde_json::json!({
                                "app_id": job.app_id,
                                "job_id": job.job_id,
                                "deployment_id": job.deployment_id,
                                "vm_id": vm_id,
                                "cpu_usage": vm_metrics.cpu_usage,
                                "ram_used_bytes": vm_metrics.ram_used_bytes,
                                "tx_bytes": vm_metrics.tx_bytes,
                                "rx_bytes": vm_metrics.rx_bytes,
                                "status": "RUNNING",
                                "ipv6_address": job.config.ipv6_address,
                            });

                            let subject = format!("mikrom.metrics.{}.{}", job.app_id, vm_id);
                            let _ = client.publish(subject, event.to_string().into()).await;
                        }

                        let host_metrics = HostMetrics {
                            cpu_usage: metrics.cpu_usage,
                            ram_used_bytes: metrics.ram_used_bytes,
                            ram_total_bytes: metrics.ram_total_bytes,
                            disk_used_bytes: metrics.disk_used_bytes,
                            disk_total_bytes: metrics.disk_total_bytes,
                            apps_count: metrics.apps_count,
                            load_avg_1: metrics.load_avg_1,
                            load_avg_5: metrics.load_avg_5,
                            load_avg_15: metrics.load_avg_15,
                            timestamp: metrics.timestamp,
                            vms: metrics
                                .vms
                                .into_iter()
                                .map(|(k, v)| {
                                    (
                                        k,
                                        crate::domain::VmMetrics {
                                            cpu_usage: v.cpu_usage,
                                            ram_used_bytes: v.ram_used_bytes,
                                            tx_bytes: v.tx_bytes,
                                            rx_bytes: v.rx_bytes,
                                        },
                                    )
                                })
                                .collect(),
                        };
                        let _ = server
                            .app_service
                            .worker_repo
                            .update_metrics(&heartbeat.host_id, host_metrics)
                            .await;
                    }
                },
                Err(e) => {
                    tracing::error!("Failed to decode heartbeat: {}", e);
                },
            }
        });
    }

    async fn handle_deploy(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            if let Ok(req) = DeployRequest::decode(&message.payload[..]) {
                tracing::info!(
                    app_id = %req.app_id,
                    deployment_id = %req.deployment_id,
                    user_id = %req.user_id,
                    "Received scheduler deploy request"
                );
                let result = server.deploy_app(req).await;
                if let Some(reply) = message.reply {
                    let response = match result {
                        Ok(resp) => {
                            tracing::info!(
                                job_id = %resp.job_id,
                                status = %resp.status,
                                "Scheduler deploy request completed"
                            );
                            resp
                        },
                        Err(e) => mikrom_proto::scheduler::DeployResponse {
                            message: e.to_string(),
                            ..Default::default()
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

    async fn handle_vm_failed(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let Ok(event) = mikrom_proto::agent::VmFailureEvent::decode(&message.payload[..])
            else {
                tracing::warn!("Failed to decode VM failure event");
                return;
            };

            let Some(job) = server
                .app_service
                .job_repo
                .find_job_by_vm_id(&event.vm_id)
                .await
                .ok()
                .flatten()
            else {
                tracing::warn!(vm_id = %event.vm_id, "VM failure event received for unknown job");
                return;
            };

            if matches!(
                job.status,
                crate::domain::JobStatus::Failed
                    | crate::domain::JobStatus::Cancelled
                    | crate::domain::JobStatus::Stopped
            ) {
                tracing::debug!(
                    job_id = %job.job_id,
                    vm_id = %event.vm_id,
                    "Ignoring VM failure event for terminal job"
                );
                return;
            }

            let message_text = if event.error_message.is_empty() {
                "VM startup failed".to_string()
            } else {
                event.error_message
            };

            tracing::error!(
                job_id = %job.job_id,
                vm_id = %event.vm_id,
                error = %message_text,
                "Received immediate VM failure event"
            );

            if let Err(e) = server
                .app_service
                .job_repo
                .fail_job(
                    &job.job_id,
                    message_text.clone(),
                    chrono::Utc::now().timestamp(),
                )
                .await
            {
                tracing::error!(
                    job_id = %job.job_id,
                    vm_id = %event.vm_id,
                    error = %e,
                    "Failed to persist VM failure event"
                );
                return;
            }

            let mut updated_job = job;
            updated_job.status = crate::domain::JobStatus::Failed;
            updated_job.stopped_at = Some(chrono::Utc::now().timestamp());
            updated_job.error_message = Some(message_text);

            if let Ok(Some(mut app)) = server
                .app_service
                .app_repo
                .get_app_config(&updated_job.app_id)
                .await
            {
                let retry_after =
                    chrono::Utc::now().timestamp() + Self::restore_retry_backoff_secs();
                app.restore_retry_after_at = retry_after;
                if let Err(e) = server.app_service.app_repo.update_app_config(app).await {
                    tracing::error!(
                        app_id = %updated_job.app_id,
                        retry_after = %retry_after,
                        error = %e,
                        "Failed to persist restore backoff after VM failure"
                    );
                }
            }

            use mikrom_proto::scheduler::AppInfo;
            use prost::Message;

            let info = AppInfo {
                job_id: updated_job.job_id.clone(),
                app_id: updated_job.app_id.clone(),
                app_name: updated_job.app_name.clone(),
                image: updated_job.image.clone(),
                status: updated_job.status as i32,
                host_id: updated_job.host_id.clone().unwrap_or_default(),
                vm_id: updated_job.vm_id.clone().unwrap_or_default(),
                user_id: updated_job.user_id.clone(),
                deployment_id: updated_job.deployment_id.clone().unwrap_or_default(),
                ipv6_address: updated_job.config.ipv6_address.clone().unwrap_or_default(),
                ..Default::default()
            };

            let mut buf = Vec::new();
            if info.encode(&mut buf).is_ok() {
                let _ = client
                    .publish(mikrom_proto::subjects::SCHEDULER_JOB_UPDATES, buf.into())
                    .await;
            }
        });
    }

    async fn handle_status(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            if let Ok(req) = AppStatusRequest::decode(&message.payload[..]) {
                let result = server.get_app_status(req).await;
                if let Some(reply) = message.reply {
                    let response = match result {
                        Ok(resp) => resp,
                        Err(e) => mikrom_proto::scheduler::AppStatusResponse {
                            error_message: e.to_string(),
                            ..Default::default()
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

    async fn handle_list_apps(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            if let Ok(req) = ListAppsRequest::decode(&message.payload[..]) {
                tracing::info!(
                    user_id = %req.user_id,
                    "Received scheduler list-apps request"
                );
                let result = server.list_apps(req).await;
                if let Some(reply) = message.reply {
                    let response = match result {
                        Ok(resp) => {
                            tracing::info!(
                                apps_count = resp.apps.len(),
                                "Scheduler list-apps request completed"
                            );
                            resp
                        },
                        Err(_) => mikrom_proto::scheduler::ListAppsResponse { apps: vec![] },
                    };
                    let mut buf = Vec::new();
                    if response.encode(&mut buf).is_ok() {
                        let _ = client.publish(reply, buf.into()).await;
                    }
                }
            }
        });
    }

    async fn handle_list_workers(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            if let Ok(req) = ListWorkersRequest::decode(&message.payload[..]) {
                let result = server.list_workers(req).await;
                if let Some(reply) = message.reply {
                    let response = match result {
                        Ok(resp) => resp,
                        Err(_) => mikrom_proto::scheduler::ListWorkersResponse { workers: vec![] },
                    };
                    let mut buf = Vec::new();
                    if response.encode(&mut buf).is_ok() {
                        let _ = client.publish(reply, buf.into()).await;
                    }
                }
            }
        });
    }

    async fn handle_pause(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            if let Ok(req) = PauseRequest::decode(&message.payload[..]) {
                let job_id = req.job_id.clone();
                tracing::info!(
                    job_id = %job_id,
                    user_id = %req.user_id,
                    "Received scheduler pause request"
                );
                let result = server.pause_app(req).await;
                if let Some(reply) = message.reply {
                    let response = match result {
                        Ok(resp) => {
                            tracing::info!(
                                job_id = %job_id,
                                success = resp.success,
                                "Scheduler pause request completed"
                            );
                            resp
                        },
                        Err(e) => mikrom_proto::scheduler::PauseResponse {
                            success: false,
                            message: e.to_string(),
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

    async fn handle_resume(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            if let Ok(req) = ResumeRequest::decode(&message.payload[..]) {
                let job_id = req.job_id.clone();
                tracing::info!(
                    job_id = %job_id,
                    user_id = %req.user_id,
                    "Received scheduler resume request"
                );
                let result = server.resume_app(req).await;
                if let Some(reply) = message.reply {
                    let response = match result {
                        Ok(resp) => {
                            tracing::info!(
                                job_id = %job_id,
                                success = resp.success,
                                "Scheduler resume request completed"
                            );
                            resp
                        },
                        Err(e) => mikrom_proto::scheduler::ResumeResponse {
                            success: false,
                            message: e.to_string(),
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

    async fn handle_cancel(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            if let Ok(req) = CancelRequest::decode(&message.payload[..]) {
                let result = server.cancel_app(req).await;
                if let Some(reply) = message.reply {
                    let response = match result {
                        Ok(resp) => resp,
                        Err(e) => mikrom_proto::scheduler::CancelResponse {
                            success: false,
                            message: e.to_string(),
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

    async fn handle_delete(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            if let Ok(req) = DeleteAppRequest::decode(&message.payload[..]) {
                tracing::info!(
                    job_id = %req.job_id,
                    user_id = %req.user_id,
                    "Received scheduler delete request"
                );
                let result = server.delete_app(req).await;
                if let Some(reply) = message.reply {
                    let response = match result {
                        Ok(resp) => {
                            tracing::info!(
                                success = resp.success,
                                "Scheduler delete request completed"
                            );
                            resp
                        },
                        Err(e) => mikrom_proto::scheduler::DeleteAppResponse {
                            success: false,
                            message: e.to_string(),
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

    async fn handle_delete_all(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            if let Ok(req) = DeleteAllByAppRequest::decode(&message.payload[..]) {
                tracing::info!(
                    app_id = %req.app_id,
                    user_id = %req.user_id,
                    "Received scheduler delete-all request"
                );
                let result = server.delete_all_by_app(req).await;
                if let Some(reply) = message.reply {
                    let response = match result {
                        Ok(resp) => {
                            tracing::info!(
                                success = resp.success,
                                "Scheduler delete-all request completed"
                            );
                            resp
                        },
                        Err(e) => mikrom_proto::scheduler::DeleteAllByAppResponse {
                            success: false,
                            message: e.to_string(),
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

    async fn handle_scale(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            use mikrom_proto::scheduler::ScaleAppRequest;
            if let Ok(req) = ScaleAppRequest::decode(&message.payload[..]) {
                tracing::info!(
                    app_id = %req.app_id,
                    desired_replicas = %req.desired_replicas,
                    user_id = %req.user_id,
                    "Received scheduler scale request"
                );
                let result = server.scale_app(req).await;
                if let Some(reply) = message.reply {
                    let response = match result {
                        Ok(resp) => {
                            tracing::info!(
                                success = resp.success,
                                "Scheduler scale request completed"
                            );
                            resp
                        },
                        Err(e) => mikrom_proto::scheduler::ScaleAppResponse {
                            success: false,
                            message: e.to_string(),
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

    async fn handle_create_volume(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            use mikrom_proto::scheduler::CreateVolumeRequest;
            if let Ok(req) = CreateVolumeRequest::decode(&message.payload[..]) {
                tracing::info!(volume_id = %req.volume_id, "Received scheduler create-volume request");
                let result = server.create_volume(req).await;
                if let Some(reply) = message.reply {
                    let mut buf = Vec::new();
                    match result {
                        Ok(resp) => {
                            if resp.encode(&mut buf).is_ok() {
                                let _ = client.publish(reply, buf.into()).await;
                            }
                        },
                        Err(e) => {
                            let resp = mikrom_proto::scheduler::CreateVolumeResponse {
                                success: false,
                                message: e.to_string(),
                            };
                            if resp.encode(&mut buf).is_ok() {
                                let _ = client.publish(reply, buf.into()).await;
                            }
                        },
                    }
                }
            }
        });
    }

    async fn handle_create_snapshot(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            use mikrom_proto::scheduler::CreateSnapshotRequest;
            if let Ok(req) = CreateSnapshotRequest::decode(&message.payload[..]) {
                tracing::info!(volume_id = %req.volume_id, snapshot_name = %req.snapshot_name, "Received scheduler create-snapshot request");
                let result = server.create_snapshot(req).await;
                if let Some(reply) = message.reply {
                    let mut buf = Vec::new();
                    match result {
                        Ok(resp) => {
                            if resp.encode(&mut buf).is_ok() {
                                let _ = client.publish(reply, buf.into()).await;
                            }
                        },
                        Err(e) => {
                            let resp = mikrom_proto::scheduler::CreateSnapshotResponse {
                                success: false,
                                message: e.to_string(),
                            };
                            if resp.encode(&mut buf).is_ok() {
                                let _ = client.publish(reply, buf.into()).await;
                            }
                        },
                    }
                }
            }
        });
    }

    async fn handle_delete_volume(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            use mikrom_proto::scheduler::DeleteVolumeRequest;
            if let Ok(req) = DeleteVolumeRequest::decode(&message.payload[..]) {
                tracing::info!(volume_id = %req.volume_id, "Received scheduler delete-volume request");
                let result = server.delete_volume(req).await;
                if let Some(reply) = message.reply {
                    let mut buf = Vec::new();
                    match result {
                        Ok(resp) => {
                            if resp.encode(&mut buf).is_ok() {
                                let _ = client.publish(reply, buf.into()).await;
                            }
                        },
                        Err(e) => {
                            let resp = mikrom_proto::scheduler::DeleteVolumeResponse {
                                success: false,
                                message: e.to_string(),
                            };
                            if resp.encode(&mut buf).is_ok() {
                                let _ = client.publish(reply, buf.into()).await;
                            }
                        },
                    }
                }
            }
        });
    }

    async fn handle_delete_snapshot(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            use mikrom_proto::scheduler::DeleteSnapshotRequest;
            if let Ok(req) = DeleteSnapshotRequest::decode(&message.payload[..]) {
                tracing::info!(volume_id = %req.volume_id, snapshot_name = %req.snapshot_name, "Received scheduler delete-snapshot request");
                let result = server.delete_snapshot(req).await;
                if let Some(reply) = message.reply {
                    let mut buf = Vec::new();
                    match result {
                        Ok(resp) => {
                            if resp.encode(&mut buf).is_ok() {
                                let _ = client.publish(reply, buf.into()).await;
                            }
                        },
                        Err(e) => {
                            let resp = mikrom_proto::scheduler::DeleteSnapshotResponse {
                                success: false,
                                message: e.to_string(),
                            };
                            if resp.encode(&mut buf).is_ok() {
                                let _ = client.publish(reply, buf.into()).await;
                            }
                        },
                    }
                }
            }
        });
    }

    async fn handle_restore_snapshot(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            use mikrom_proto::scheduler::RestoreSnapshotRequest;
            if let Ok(req) = RestoreSnapshotRequest::decode(&message.payload[..]) {
                tracing::info!(volume_id = %req.volume_id, snapshot_name = %req.snapshot_name, "Received scheduler restore-snapshot request");
                let result = server.restore_snapshot(req).await;
                if let Some(reply) = message.reply {
                    let mut buf = Vec::new();
                    match result {
                        Ok(resp) => {
                            if resp.encode(&mut buf).is_ok() {
                                let _ = client.publish(reply, buf.into()).await;
                            }
                        },
                        Err(e) => {
                            let resp = mikrom_proto::scheduler::RestoreSnapshotResponse {
                                success: false,
                                message: e.to_string(),
                            };
                            if resp.encode(&mut buf).is_ok() {
                                let _ = client.publish(reply, buf.into()).await;
                            }
                        },
                    }
                }
            }
        });
    }

    async fn handle_clone_volume(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            use mikrom_proto::scheduler::CloneVolumeRequest;
            if let Ok(req) = CloneVolumeRequest::decode(&message.payload[..]) {
                tracing::info!(source_volume_id = %req.source_volume_id, target_volume_id = %req.target_volume_id, "Received scheduler clone-volume request");
                let result = server.clone_volume(req).await;
                if let Some(reply) = message.reply {
                    let mut buf = Vec::new();
                    match result {
                        Ok(resp) => {
                            if resp.encode(&mut buf).is_ok() {
                                let _ = client.publish(reply, buf.into()).await;
                            }
                        },
                        Err(e) => {
                            let resp = mikrom_proto::scheduler::CloneVolumeResponse {
                                success: false,
                                message: e.to_string(),
                            };
                            if resp.encode(&mut buf).is_ok() {
                                let _ = client.publish(reply, buf.into()).await;
                            }
                        },
                    }
                }
            }
        });
    }
}

struct RouterRestoreGuard {
    router_restore_in_progress: Arc<Mutex<HashSet<String>>>,
    app_id: String,
}

impl Drop for RouterRestoreGuard {
    fn drop(&mut self) {
        if let Ok(mut in_progress) = self.router_restore_in_progress.lock() {
            in_progress.remove(&self.app_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::AppService;
    use crate::application::deployment::DeploymentService;
    use crate::domain::worker::{MockAgentClient, MockJobRepository, MockWorkerRepository, Worker};
    use chrono::Utc;
    use sqlx::PgPool;
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    async fn connect_nats_or_skip() -> Option<async_nats::Client> {
        match async_nats::connect("nats://localhost:4223").await {
            Ok(client) => Some(client),
            Err(err) => {
                eprintln!("Skipping scheduler event loop test: failed to connect to NATS: {err}");
                None
            },
        }
    }

    #[tokio::test]
    async fn test_calculate_mesh_updates_prunes_dead_workers() {
        let mut worker_repo = MockWorkerRepository::new();
        let mut job_repo = MockJobRepository::new();

        let now = Utc::now().timestamp();

        // Active worker (10 seconds ago)
        let active_worker = Worker {
            host_id: "active-host".to_string(),
            hostname: "active".to_string(),
            advertise_address: "1.1.1.1".to_string(),
            wireguard_pubkey: Some("pub-active".to_string()),
            wireguard_ip: Some("fd00::1".to_string()),
            wireguard_port: Some(51820),
            metrics: None,
            registered_at: now,
            last_heartbeat: now - 10,
        };

        // Dead worker (60 seconds ago)
        let dead_worker = Worker {
            host_id: "dead-host".to_string(),
            hostname: "dead".to_string(),
            advertise_address: "2.2.2.2".to_string(),
            wireguard_pubkey: Some("pub-dead".to_string()),
            wireguard_ip: Some("fd00::2".to_string()),
            wireguard_port: Some(51820),
            metrics: None,
            registered_at: now,
            last_heartbeat: now - 60,
        };

        // Mock list_workers to return both
        worker_repo
            .expect_list_workers()
            .returning(move || Ok(vec![active_worker.clone(), dead_worker.clone()]));

        job_repo.expect_list_jobs().returning(|_, _, _| Ok(vec![]));

        let url = "postgres://localhost/dummy";
        let pool = PgPool::connect_lazy(url).unwrap();

        let worker_repo = Arc::new(worker_repo);
        let job_repo = Arc::new(job_repo);
        let agent_client = Arc::new(MockAgentClient::new());

        let Some(nats_client) = connect_nats_or_skip().await else {
            return;
        };
        let deployment = DeploymentService::new(
            job_repo.clone(),
            worker_repo.clone(),
            agent_client.clone(),
            nats_client.clone(),
        );

        // Dummy AppRepo for tests
        #[derive(Debug)]
        struct DummyAppRepo;
        #[async_trait::async_trait]
        impl crate::domain::AppRepository for DummyAppRepo {
            async fn update_app_config(&self, _: crate::domain::AppConfig) -> anyhow::Result<()> {
                Ok(())
            }
            async fn get_app_config(
                &self,
                _: &str,
            ) -> anyhow::Result<Option<crate::domain::AppConfig>> {
                Ok(None)
            }
            async fn get_app_config_by_hostname(
                &self,
                _: &str,
            ) -> anyhow::Result<Option<crate::domain::AppConfig>> {
                Ok(None)
            }
            async fn list_all_apps(&self) -> anyhow::Result<Vec<crate::domain::AppConfig>> {
                Ok(vec![])
            }
            async fn list_autoscaling_apps(&self) -> anyhow::Result<Vec<crate::domain::AppConfig>> {
                Ok(vec![])
            }
            async fn remove_app_config(&self, _: &str) -> anyhow::Result<()> {
                Ok(())
            }
        }

        let app_service = Arc::new(AppService {
            deployment,
            worker_repo,
            job_repo,
            app_repo: Arc::new(DummyAppRepo),
            agent_client,
            nats_client,
            pool,
            router_idle_timeout_secs: 900,
        });

        let server = SchedulerServer {
            app_service,
            certs: None,
        };

        let updates = NatsEventLoop::calculate_mesh_updates(&server).await;

        // Verify:
        // 1. Both workers should get an update (because they are in the list)
        // 2. BUT the dead worker should NOT be in the peer list of the active worker

        let active_update = updates
            .get("active-host")
            .expect("active host update missing");

        let has_dead_peer = active_update.peers.iter().any(|p| p.host_id == "dead-host");
        assert!(
            !has_dead_peer,
            "Dead worker should have been pruned from the mesh"
        );

        assert_eq!(
            updates.len(),
            2,
            "Both workers should still be updated (though they might be alone)"
        );
    }

    #[test]
    fn test_router_restore_guard_deduplicates_app_restores() {
        let in_progress = Arc::new(Mutex::new(HashSet::new()));

        let guard = NatsEventLoop::acquire_router_restore_guard(&in_progress, "app-1");
        assert!(guard.is_some());
        assert!(NatsEventLoop::acquire_router_restore_guard(&in_progress, "app-1").is_none());

        drop(guard);

        let guard = NatsEventLoop::acquire_router_restore_guard(&in_progress, "app-1");
        assert!(guard.is_some());
    }

    #[test]
    fn test_restore_retry_backoff_defaults_and_parses_env_value() {
        assert_eq!(
            NatsEventLoop::restore_retry_backoff_secs_from(None),
            DEFAULT_RESTORE_RETRY_BACKOFF_SECS
        );
        assert_eq!(
            NatsEventLoop::restore_retry_backoff_secs_from(Some("0")),
            DEFAULT_RESTORE_RETRY_BACKOFF_SECS
        );
        assert_eq!(
            NatsEventLoop::restore_retry_backoff_secs_from(Some("1800")),
            1800
        );
    }
}
