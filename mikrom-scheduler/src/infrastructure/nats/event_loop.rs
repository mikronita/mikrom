use super::subjects;
use crate::server::SchedulerServer;
use futures::StreamExt;
use mikrom_proto::scheduler::{
    AppStatusRequest, AppStatusResponse, CancelRequest, CancelResponse, DeleteAllByAppRequest,
    DeleteAllByAppResponse, DeleteAppRequest, DeleteAppResponse, DeployRequest, DeployResponse,
    ListAppsRequest, ListAppsResponse, ListWorkersRequest, ListWorkersResponse, PauseRequest,
    PauseResponse, ResumeRequest, ResumeResponse, RouterHeartbeat, WorkerHeartbeat,
};
use prost::Message;
use std::collections::HashSet;
use std::fmt::Display;
use std::future::Future;
use std::sync::{Arc, Mutex};

const MESH_PRUNING_THRESHOLD_SECS: i64 = 30;

async fn publish_best_effort(
    client: &async_nats::Client,
    subject: impl Into<String>,
    payload: Vec<u8>,
    context: &'static str,
) {
    let subject = subject.into();
    if let Err(e) = client.publish(subject.clone(), payload.into()).await {
        tracing::warn!(
            %context,
            %subject,
            error = %e,
            "Failed to publish NATS message"
        );
    }
}

async fn publish_response_best_effort<T: Message>(
    client: &async_nats::Client,
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

    publish_best_effort(client, reply.to_string(), buf, context).await;
}

async fn dispatch_request<TReq, TResp, F, Fut, E>(
    server: SchedulerServer,
    client: async_nats::Client,
    message: async_nats::Message,
    event: &'static str,
    reply_context: &'static str,
    handler: F,
    error_response: impl FnOnce(E) -> TResp,
) where
    TReq: Message + Default + Send + 'static,
    TResp: Message + Send + 'static,
    F: FnOnce(TReq) -> Fut + Send + 'static,
    Fut: Future<Output = Result<TResp, E>> + Send,
    E: Display,
{
    let subject = message.subject.clone();
    let req = match TReq::decode(&message.payload[..]) {
        Ok(req) => req,
        Err(e) => {
            tracing::warn!(
                %event,
                %subject,
                error = %e,
                "Failed to decode NATS request"
            );
            return;
        },
    };

    let telemetry = server.app_service.context.telemetry.clone();
    let result = telemetry
        .observe_result("nats", event, async { handler(req).await })
        .await;

    let Some(reply) = message.reply else {
        tracing::warn!(
            %event,
            %subject,
            "NATS request did not include a reply subject"
        );
        return;
    };

    let response = match result {
        Ok(resp) => resp,
        Err(e) => error_response(e),
    };

    publish_response_best_effort(&client, reply, &response, reply_context).await;
}

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

    pub async fn run(self) -> anyhow::Result<()> {
        let client = self.client.clone();
        let queue_group = self.queue_group.clone();

        // 1. Heartbeats
        let mut heartbeat_sub = client
            .subscribe(mikrom_proto::subjects::SCHEDULER_WORKER_HEARTBEAT)
            .await?;

        let mut router_heartbeat_sub = client
            .subscribe(mikrom_proto::subjects::SCHEDULER_ROUTER_HEARTBEAT)
            .await?;
        let mut router_traffic_sub = client
            .queue_subscribe(
                mikrom_proto::subjects::ROUTER_TRAFFIC_EVENT,
                queue_group.clone(),
            )
            .await?;

        // 2. Queue Group Subscriptions for Load Balancing
        let mut deploy_sub = client
            .queue_subscribe(
                mikrom_proto::subjects::SCHEDULER_DEPLOY,
                queue_group.clone(),
            )
            .await?;
        let mut status_sub = client
            .queue_subscribe(
                mikrom_proto::subjects::SCHEDULER_GET_JOB,
                queue_group.clone(),
            )
            .await?;
        let mut list_sub = client
            .queue_subscribe(
                mikrom_proto::subjects::SCHEDULER_LIST_APPS,
                queue_group.clone(),
            )
            .await?;
        let mut list_workers_sub = client
            .queue_subscribe(
                mikrom_proto::subjects::SCHEDULER_LIST_WORKERS,
                queue_group.clone(),
            )
            .await?;
        let mut pause_sub = client
            .queue_subscribe(
                mikrom_proto::subjects::SCHEDULER_PAUSE_APP,
                queue_group.clone(),
            )
            .await?;
        let mut resume_sub = client
            .queue_subscribe(
                mikrom_proto::subjects::SCHEDULER_RESUME_APP,
                queue_group.clone(),
            )
            .await?;
        let mut cancel_sub = client
            .queue_subscribe(
                mikrom_proto::subjects::SCHEDULER_CANCEL_APP,
                queue_group.clone(),
            )
            .await?;
        let mut delete_sub = client
            .queue_subscribe(
                mikrom_proto::subjects::SCHEDULER_DELETE_APP,
                queue_group.clone(),
            )
            .await?;
        let mut delete_all_sub = client
            .queue_subscribe(subjects::DELETE_ALL_BY_APP, queue_group.clone())
            .await?;
        let mut scale_sub = client
            .queue_subscribe(
                mikrom_proto::subjects::SCHEDULER_SCALE_APP,
                queue_group.clone(),
            )
            .await?;
        let mut health_sub = client
            .queue_subscribe(subjects::CHECK_HEALTH, queue_group.clone())
            .await?;
        let mut security_sub = client
            .queue_subscribe(subjects::UPDATE_SECURITY_GROUPS, queue_group.clone())
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
            .queue_subscribe(subjects::CREATE_VOLUME, queue_group.clone())
            .await?;
        let mut snapshot_sub = client
            .queue_subscribe(subjects::CREATE_SNAPSHOT, queue_group.clone())
            .await?;
        let mut delete_volume_sub = client
            .queue_subscribe(subjects::DELETE_VOLUME, queue_group.clone())
            .await?;
        let mut delete_snapshot_sub = client
            .queue_subscribe(subjects::DELETE_SNAPSHOT, queue_group.clone())
            .await?;
        let mut restore_snapshot_sub = client
            .queue_subscribe(subjects::RESTORE_SNAPSHOT, queue_group.clone())
            .await?;
        let mut clone_volume_sub = client
            .queue_subscribe(subjects::CLONE_VOLUME, queue_group.clone())
            .await?;
        let mut vm_snapshot_create_sub = client
            .queue_subscribe(subjects::VM_SNAPSHOT_CREATE, queue_group.clone())
            .await?;
        let mut vm_snapshot_restore_sub = client
            .queue_subscribe(subjects::VM_SNAPSHOT_RESTORE, queue_group.clone())
            .await?;
        let mut vm_snapshot_delete_sub = client
            .queue_subscribe(subjects::VM_SNAPSHOT_DELETE, queue_group.clone())
            .await?;
        let mut vm_snapshot_list_sub = client
            .queue_subscribe(subjects::VM_SNAPSHOT_LIST, queue_group.clone())
            .await?;
        let mut attach_volume_sub = client
            .queue_subscribe(subjects::ATTACH_VOLUME, queue_group.clone())
            .await?;
        let mut detach_volume_sub = client
            .queue_subscribe(subjects::DETACH_VOLUME, queue_group.clone())
            .await?;
        let mut start_migration_sub = client
            .queue_subscribe(subjects::START_MIGRATION, queue_group.clone())
            .await?;
        let mut cancel_migration_sub = client
            .queue_subscribe(subjects::CANCEL_MIGRATION, queue_group.clone())
            .await?;
        let mut query_migration_sub = client
            .queue_subscribe(subjects::QUERY_MIGRATION, queue_group.clone())
            .await?;
        let mut set_balloon_sub = client
            .queue_subscribe(subjects::SET_BALLOON, queue_group.clone())
            .await?;
        let mut query_balloon_sub = client
            .queue_subscribe(subjects::QUERY_BALLOON, queue_group.clone())
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
                Some(msg) = vm_snapshot_create_sub.next() => self.handle_vm_snapshot_create(msg).await,
                Some(msg) = vm_snapshot_restore_sub.next() => self.handle_vm_snapshot_restore(msg).await,
                Some(msg) = vm_snapshot_delete_sub.next() => self.handle_vm_snapshot_delete(msg).await,
                Some(msg) = vm_snapshot_list_sub.next() => self.handle_vm_snapshot_list(msg).await,
                Some(msg) = attach_volume_sub.next() => self.handle_attach_volume(msg).await,
                Some(msg) = detach_volume_sub.next() => self.handle_detach_volume(msg).await,
                Some(msg) = start_migration_sub.next() => self.handle_start_migration(msg).await,
                Some(msg) = cancel_migration_sub.next() => self.handle_cancel_migration(msg).await,
                Some(msg) = query_migration_sub.next() => self.handle_query_migration(msg).await,
                Some(msg) = set_balloon_sub.next() => self.handle_set_balloon(msg).await,
                Some(msg) = query_balloon_sub.next() => self.handle_query_balloon(msg).await,
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
                publish_best_effort(&client, subject, buf, "mesh-update").await;
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
                    host_id: w.host_id.to_string(),
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
                .filter(|p| p.host_id != w.host_id.to_string())
                .cloned()
                .collect();

            updates.insert(
                w.host_id.to_string(),
                mikrom_proto::scheduler::NetworkMeshUpdate { peers },
            );
        }

        updates
    }

    async fn handle_update_security_groups(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<
                mikrom_proto::scheduler::UpdateSecurityGroupsRequest,
                mikrom_proto::scheduler::UpdateSecurityGroupsResponse,
                _,
                _,
                anyhow::Error,
            >(
                server,
                client,
                message,
                "update_security_groups",
                "scheduler-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.update_security_groups(req).await }
                },
                |e| mikrom_proto::scheduler::UpdateSecurityGroupsResponse {
                    success: false,
                    message: e.to_string(),
                },
            )
            .await;
        });
    }

    async fn handle_update_app_scaling_config(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<
                mikrom_proto::scheduler::UpdateAppScalingConfigRequest,
                mikrom_proto::scheduler::UpdateAppScalingConfigResponse,
                _,
                _,
                anyhow::Error,
            >(
                server,
                client,
                message,
                "update_app_scaling_config",
                "scheduler-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.update_app_scaling_config(req).await }
                },
                |e| mikrom_proto::scheduler::UpdateAppScalingConfigResponse {
                    success: false,
                    message: e.to_string(),
                },
            )
            .await;
        });
    }

    async fn handle_check_health(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<
                mikrom_proto::scheduler::CheckHealthRequest,
                mikrom_proto::scheduler::CheckHealthResponse,
                _,
                _,
                anyhow::Error,
            >(
                server,
                client,
                message,
                "check_health",
                "scheduler-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.check_health(req).await }
                },
                |e| mikrom_proto::scheduler::CheckHealthResponse {
                    is_healthy: false,
                    message: e.to_string(),
                },
            )
            .await;
        });
    }

    async fn handle_router_heartbeat(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            match RouterHeartbeat::decode(&message.payload[..]) {
                Ok(heartbeat) => {
                    let telemetry = server.app_service.context.telemetry.clone();
                    if let Err(e) = telemetry
                        .observe_result("nats", "router_heartbeat", async {
                            server.app_service.process_router_heartbeat(heartbeat).await
                        })
                        .await
                    {
                        tracing::error!("Failed to process router heartbeat: {}", e);
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
                    // Fetch app ID for deduplication guard
                    let app_id = match server
                        .app_service
                        .app_repo
                        .get_app_config_by_hostname(&event.hostname)
                        .await
                    {
                        Ok(Some(app)) => app.id,
                        _ => return,
                    };

                    let Some(_restore_guard) =
                        Self::acquire_router_restore_guard(&router_restore_in_progress, &app_id)
                    else {
                        return;
                    };

                    let telemetry = server.app_service.context.telemetry.clone();
                    if let Err(e) = telemetry
                        .observe_result("nats", "router_traffic", async {
                            server.app_service.process_router_traffic(event).await
                        })
                        .await
                    {
                        tracing::error!("Failed to process router traffic: {}", e);
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
                    let telemetry = server.app_service.context.telemetry.clone();
                    if let Err(e) = telemetry
                        .observe_result("nats", "worker_heartbeat", async {
                            server.app_service.process_worker_heartbeat(heartbeat).await
                        })
                        .await
                    {
                        tracing::error!("Failed to process worker heartbeat: {}", e);
                    } else {
                        // Fast-path: Broadcast mesh update immediately on new registration
                        Self::broadcast_mesh_updates(server.clone(), client.clone()).await;
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
            let handler_server = server.clone();
            dispatch_request::<DeployRequest, DeployResponse, _, _, anyhow::Error>(
                server,
                client,
                message,
                "deploy",
                "deploy-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.deploy_app(req).await }
                },
                |e| DeployResponse {
                    message: e.to_string(),
                    ..Default::default()
                },
            )
            .await;
        });
    }

    async fn handle_vm_failed(&self, message: async_nats::Message) {
        let server = self.server.clone();
        tokio::spawn(async move {
            if let Ok(event) = mikrom_proto::agent::VmFailureEvent::decode(&message.payload[..]) {
                let telemetry = server.app_service.context.telemetry.clone();
                if let Err(e) = telemetry
                    .observe_result("nats", "vm_failure", async {
                        server.app_service.process_vm_failure(event).await
                    })
                    .await
                {
                    tracing::error!("Failed to process VM failure event: {}", e);
                }
            } else {
                tracing::warn!("Failed to decode VM failure event");
            }
        });
    }

    async fn handle_status(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<AppStatusRequest, AppStatusResponse, _, _, anyhow::Error>(
                server,
                client,
                message,
                mikrom_proto::subjects::SCHEDULER_GET_JOB,
                "status-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.get_app_status(req).await }
                },
                |e| AppStatusResponse {
                    error_message: e.to_string(),
                    ..Default::default()
                },
            )
            .await;
        });
    }

    async fn handle_list_apps(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<ListAppsRequest, ListAppsResponse, _, _, anyhow::Error>(
                server,
                client,
                message,
                mikrom_proto::subjects::SCHEDULER_LIST_APPS,
                "list-apps-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.list_apps(req).await }
                },
                |_| ListAppsResponse { apps: vec![] },
            )
            .await;
        });
    }

    async fn handle_list_workers(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<ListWorkersRequest, ListWorkersResponse, _, _, anyhow::Error>(
                server,
                client,
                message,
                mikrom_proto::subjects::SCHEDULER_LIST_WORKERS,
                "list-workers-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.list_workers(req).await }
                },
                |_| ListWorkersResponse { workers: vec![] },
            )
            .await;
        });
    }

    async fn handle_pause(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<PauseRequest, PauseResponse, _, _, anyhow::Error>(
                server,
                client,
                message,
                mikrom_proto::subjects::SCHEDULER_PAUSE_APP,
                "pause-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.pause_app(req).await }
                },
                |e| PauseResponse {
                    success: false,
                    message: e.to_string(),
                },
            )
            .await;
        });
    }

    async fn handle_resume(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<ResumeRequest, ResumeResponse, _, _, anyhow::Error>(
                server,
                client,
                message,
                mikrom_proto::subjects::SCHEDULER_RESUME_APP,
                "resume-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.resume_app(req).await }
                },
                |e| ResumeResponse {
                    success: false,
                    message: e.to_string(),
                },
            )
            .await;
        });
    }

    async fn handle_cancel(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<CancelRequest, CancelResponse, _, _, anyhow::Error>(
                server,
                client,
                message,
                mikrom_proto::subjects::SCHEDULER_CANCEL_APP,
                "cancel-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.cancel_app(req).await }
                },
                |e| CancelResponse {
                    success: false,
                    message: e.to_string(),
                },
            )
            .await;
        });
    }

    async fn handle_delete(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<DeleteAppRequest, DeleteAppResponse, _, _, anyhow::Error>(
                server,
                client,
                message,
                mikrom_proto::subjects::SCHEDULER_DELETE_APP,
                "delete-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.delete_app(req).await }
                },
                |e| DeleteAppResponse {
                    success: false,
                    message: e.to_string(),
                },
            )
            .await;
        });
    }

    async fn handle_delete_all(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<DeleteAllByAppRequest, DeleteAllByAppResponse, _, _, anyhow::Error>(
                server,
                client,
                message,
                subjects::DELETE_ALL_BY_APP,
                "delete-all-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.delete_all_by_app(req).await }
                },
                |e| DeleteAllByAppResponse {
                    success: false,
                    message: e.to_string(),
                },
            )
            .await;
        });
    }

    async fn handle_scale(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<
                mikrom_proto::scheduler::ScaleAppRequest,
                mikrom_proto::scheduler::ScaleAppResponse,
                _,
                _,
                anyhow::Error,
            >(
                server,
                client,
                message,
                mikrom_proto::subjects::SCHEDULER_SCALE_APP,
                "scale-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.scale_app(req).await }
                },
                |e| mikrom_proto::scheduler::ScaleAppResponse {
                    success: false,
                    message: e.to_string(),
                },
            )
            .await;
        });
    }

    async fn handle_create_volume(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<
                mikrom_proto::scheduler::CreateVolumeRequest,
                mikrom_proto::scheduler::CreateVolumeResponse,
                _,
                _,
                anyhow::Error,
            >(
                server,
                client,
                message,
                subjects::CREATE_VOLUME,
                "create-volume-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.create_volume(req).await }
                },
                |e| mikrom_proto::scheduler::CreateVolumeResponse {
                    success: false,
                    message: e.to_string(),
                },
            )
            .await;
        });
    }

    async fn handle_create_snapshot(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<
                mikrom_proto::scheduler::CreateSnapshotRequest,
                mikrom_proto::scheduler::CreateSnapshotResponse,
                _,
                _,
                anyhow::Error,
            >(
                server,
                client,
                message,
                subjects::CREATE_SNAPSHOT,
                "create-snapshot-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.create_snapshot(req).await }
                },
                |e| mikrom_proto::scheduler::CreateSnapshotResponse {
                    success: false,
                    message: e.to_string(),
                },
            )
            .await;
        });
    }

    async fn handle_delete_volume(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<
                mikrom_proto::scheduler::DeleteVolumeRequest,
                mikrom_proto::scheduler::DeleteVolumeResponse,
                _,
                _,
                anyhow::Error,
            >(
                server,
                client,
                message,
                subjects::DELETE_VOLUME,
                "delete-volume-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.delete_volume(req).await }
                },
                |e| mikrom_proto::scheduler::DeleteVolumeResponse {
                    success: false,
                    message: e.to_string(),
                },
            )
            .await;
        });
    }

    async fn handle_delete_snapshot(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<
                mikrom_proto::scheduler::DeleteSnapshotRequest,
                mikrom_proto::scheduler::DeleteSnapshotResponse,
                _,
                _,
                anyhow::Error,
            >(
                server,
                client,
                message,
                subjects::DELETE_SNAPSHOT,
                "delete-snapshot-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.delete_snapshot(req).await }
                },
                |e| mikrom_proto::scheduler::DeleteSnapshotResponse {
                    success: false,
                    message: e.to_string(),
                },
            )
            .await;
        });
    }

    async fn handle_restore_snapshot(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<
                mikrom_proto::scheduler::RestoreSnapshotRequest,
                mikrom_proto::scheduler::RestoreSnapshotResponse,
                _,
                _,
                anyhow::Error,
            >(
                server,
                client,
                message,
                subjects::RESTORE_SNAPSHOT,
                "restore-snapshot-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.restore_snapshot(req).await }
                },
                |e| mikrom_proto::scheduler::RestoreSnapshotResponse {
                    success: false,
                    message: e.to_string(),
                },
            )
            .await;
        });
    }

    async fn handle_clone_volume(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<
                mikrom_proto::scheduler::CloneVolumeRequest,
                mikrom_proto::scheduler::CloneVolumeResponse,
                _,
                _,
                anyhow::Error,
            >(
                server,
                client,
                message,
                subjects::CLONE_VOLUME,
                "clone-volume-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.clone_volume(req).await }
                },
                |e| mikrom_proto::scheduler::CloneVolumeResponse {
                    success: false,
                    message: e.to_string(),
                },
            )
            .await;
        });
    }

    async fn handle_vm_snapshot_create(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<
                mikrom_proto::scheduler::VmSnapshotCreateRequest,
                mikrom_proto::scheduler::VmSnapshotCreateResponse,
                _,
                _,
                anyhow::Error,
            >(
                server,
                client,
                message,
                subjects::VM_SNAPSHOT_CREATE,
                "vm-snapshot-create-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.vm_snapshot_create(req).await }
                },
                |e| mikrom_proto::scheduler::VmSnapshotCreateResponse {
                    success: false,
                    message: e.to_string(),
                },
            )
            .await;
        });
    }

    async fn handle_vm_snapshot_restore(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<
                mikrom_proto::scheduler::VmSnapshotRestoreRequest,
                mikrom_proto::scheduler::VmSnapshotRestoreResponse,
                _,
                _,
                anyhow::Error,
            >(
                server,
                client,
                message,
                subjects::VM_SNAPSHOT_RESTORE,
                "vm-snapshot-restore-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.vm_snapshot_restore(req).await }
                },
                |e| mikrom_proto::scheduler::VmSnapshotRestoreResponse {
                    success: false,
                    message: e.to_string(),
                },
            )
            .await;
        });
    }

    async fn handle_vm_snapshot_delete(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<
                mikrom_proto::scheduler::VmSnapshotDeleteRequest,
                mikrom_proto::scheduler::VmSnapshotDeleteResponse,
                _,
                _,
                anyhow::Error,
            >(
                server,
                client,
                message,
                subjects::VM_SNAPSHOT_DELETE,
                "vm-snapshot-delete-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.vm_snapshot_delete(req).await }
                },
                |e| mikrom_proto::scheduler::VmSnapshotDeleteResponse {
                    success: false,
                    message: e.to_string(),
                },
            )
            .await;
        });
    }

    async fn handle_vm_snapshot_list(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<
                mikrom_proto::scheduler::VmSnapshotListRequest,
                mikrom_proto::scheduler::VmSnapshotListResponse,
                _,
                _,
                anyhow::Error,
            >(
                server,
                client,
                message,
                subjects::VM_SNAPSHOT_LIST,
                "vm-snapshot-list-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.vm_snapshot_list(req).await }
                },
                |e| mikrom_proto::scheduler::VmSnapshotListResponse {
                    success: false,
                    message: e.to_string(),
                    snapshots: vec![],
                },
            )
            .await;
        });
    }

    async fn handle_attach_volume(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<
                mikrom_proto::scheduler::AttachVolumeRequest,
                mikrom_proto::scheduler::AttachVolumeResponse,
                _,
                _,
                anyhow::Error,
            >(
                server,
                client,
                message,
                subjects::ATTACH_VOLUME,
                "attach-volume-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.attach_volume(req).await }
                },
                |e| mikrom_proto::scheduler::AttachVolumeResponse {
                    success: false,
                    message: e.to_string(),
                },
            )
            .await;
        });
    }

    async fn handle_detach_volume(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<
                mikrom_proto::scheduler::DetachVolumeRequest,
                mikrom_proto::scheduler::DetachVolumeResponse,
                _,
                _,
                anyhow::Error,
            >(
                server,
                client,
                message,
                subjects::DETACH_VOLUME,
                "detach-volume-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.detach_volume(req).await }
                },
                |e| mikrom_proto::scheduler::DetachVolumeResponse {
                    success: false,
                    message: e.to_string(),
                },
            )
            .await;
        });
    }

    async fn handle_start_migration(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<
                mikrom_proto::scheduler::StartMigrationRequest,
                mikrom_proto::scheduler::StartMigrationResponse,
                _,
                _,
                anyhow::Error,
            >(
                server,
                client,
                message,
                subjects::START_MIGRATION,
                "start-migration-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.start_migration(req).await }
                },
                |e| mikrom_proto::scheduler::StartMigrationResponse {
                    success: false,
                    message: e.to_string(),
                },
            )
            .await;
        });
    }

    async fn handle_cancel_migration(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<
                mikrom_proto::scheduler::CancelMigrationRequest,
                mikrom_proto::scheduler::CancelMigrationResponse,
                _,
                _,
                anyhow::Error,
            >(
                server,
                client,
                message,
                subjects::CANCEL_MIGRATION,
                "cancel-migration-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.cancel_migration(req).await }
                },
                |e| mikrom_proto::scheduler::CancelMigrationResponse {
                    success: false,
                    message: e.to_string(),
                },
            )
            .await;
        });
    }

    async fn handle_query_migration(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<
                mikrom_proto::scheduler::QueryMigrationRequest,
                mikrom_proto::scheduler::QueryMigrationResponse,
                _,
                _,
                anyhow::Error,
            >(
                server,
                client,
                message,
                subjects::QUERY_MIGRATION,
                "query-migration-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.query_migration(req).await }
                },
                |e| mikrom_proto::scheduler::QueryMigrationResponse {
                    success: false,
                    message: e.to_string(),
                    status: "".to_string(),
                    total_bytes: 0,
                    transferred_bytes: 0,
                    remaining_bytes: 0,
                },
            )
            .await;
        });
    }

    async fn handle_set_balloon(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<
                mikrom_proto::scheduler::SetBalloonRequest,
                mikrom_proto::scheduler::SetBalloonResponse,
                _,
                _,
                anyhow::Error,
            >(
                server,
                client,
                message,
                subjects::SET_BALLOON,
                "set-balloon-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.set_balloon(req).await }
                },
                |e| mikrom_proto::scheduler::SetBalloonResponse {
                    success: false,
                    message: e.to_string(),
                },
            )
            .await;
        });
    }

    async fn handle_query_balloon(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let handler_server = server.clone();
            dispatch_request::<
                mikrom_proto::scheduler::QueryBalloonRequest,
                mikrom_proto::scheduler::QueryBalloonResponse,
                _,
                _,
                anyhow::Error,
            >(
                server,
                client,
                message,
                subjects::QUERY_BALLOON,
                "query-balloon-reply",
                move |req| {
                    let handler_server = handler_server.clone();
                    async move { handler_server.query_balloon(req).await }
                },
                |e| mikrom_proto::scheduler::QueryBalloonResponse {
                    success: false,
                    message: e.to_string(),
                    actual_memory_mib: 0,
                    max_memory_mib: 0,
                },
            )
            .await;
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
    use crate::application::{AppService, SchedulerRuntimeConfig};
    use crate::domain::worker::{
        MockAgentClient, MockJobRepository, MockWorkerRepository, Worker, WorkerStatus,
    };
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

    fn test_runtime() -> SchedulerRuntimeConfig {
        SchedulerRuntimeConfig {
            router_idle_timeout_secs: 900,
            worker_stale_threshold_secs: 60,
            restore_retry_backoff_secs: 3600,
        }
    }

    #[tokio::test]
    async fn test_calculate_mesh_updates_prunes_dead_workers() {
        let mut worker_repo = MockWorkerRepository::new();
        let mut job_repo = MockJobRepository::new();

        let now = Utc::now().timestamp();

        // Active worker (10 seconds ago)
        let active_worker = Worker {
            host_id: crate::domain::HostId::from("active-host"),
            hostname: "active".to_string(),
            advertise_address: "1.1.1.1".to_string(),
            wireguard_pubkey: Some("pub-active".to_string()),
            wireguard_ip: Some("fd00::1".to_string()),
            wireguard_port: Some(51820),
            metrics: None,
            registered_at: now,
            last_heartbeat: now - 10,
            status: WorkerStatus::Online,
            supported_hypervisors: vec![],
        };

        // Dead worker (60 seconds ago)
        let dead_worker = Worker {
            host_id: crate::domain::HostId::from("dead-host"),
            hostname: "dead".to_string(),
            advertise_address: "2.2.2.2".to_string(),
            wireguard_pubkey: Some("pub-dead".to_string()),
            wireguard_ip: Some("fd00::2".to_string()),
            wireguard_port: Some(51820),
            metrics: None,
            registered_at: now,
            last_heartbeat: now - 60,
            status: WorkerStatus::Offline,
            supported_hypervisors: vec![],
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

            async fn remove_app_and_jobs_by_app(&self, _: &str) -> anyhow::Result<()> {
                Ok(())
            }
        }

        let app_service = Arc::new(AppService::new(
            job_repo,
            Arc::new(DummyAppRepo),
            worker_repo,
            agent_client,
            nats_client,
            pool,
            test_runtime(),
        ));

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
}
