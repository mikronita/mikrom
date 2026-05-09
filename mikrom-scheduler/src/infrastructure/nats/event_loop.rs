use crate::domain::{HostMetrics, Worker};
use crate::server::SchedulerServer;
use futures::StreamExt;
use mikrom_proto::scheduler::{
    AppStatusRequest, CancelRequest, DeleteAllByAppRequest, DeleteAppRequest, DeployRequest,
    ListAppsRequest, ListWorkersRequest, PauseRequest, ResumeRequest, RouterHeartbeat,
    WorkerHeartbeat,
};
use prost::Message;

pub struct NatsEventLoop {
    server: SchedulerServer,
    client: async_nats::Client,
}

impl NatsEventLoop {
    pub fn new(server: SchedulerServer, client: async_nats::Client) -> Self {
        Self { server, client }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let client = self.client.clone();

        // 1. Heartbeats
        let mut heartbeat_sub = client
            .subscribe("mikrom.scheduler.worker.heartbeat")
            .await?;

        let mut router_heartbeat_sub = client
            .subscribe("mikrom.scheduler.router.heartbeat")
            .await?;

        // 2. Queue Group Subscriptions for Load Balancing
        let mut deploy_sub = client
            .queue_subscribe("mikrom.scheduler.deploy", "schedulers".to_string())
            .await?;
        let mut status_sub = client
            .queue_subscribe("mikrom.scheduler.get_job", "schedulers".to_string())
            .await?;
        let mut list_sub = client
            .queue_subscribe("mikrom.scheduler.list_apps", "schedulers".to_string())
            .await?;
        let mut list_workers_sub = client
            .queue_subscribe("mikrom.scheduler.list_workers", "schedulers".to_string())
            .await?;
        let mut pause_sub = client
            .queue_subscribe("mikrom.scheduler.pause_app", "schedulers".to_string())
            .await?;
        let mut resume_sub = client
            .queue_subscribe("mikrom.scheduler.resume_app", "schedulers".to_string())
            .await?;
        let mut cancel_sub = client
            .queue_subscribe("mikrom.scheduler.cancel_app", "schedulers".to_string())
            .await?;
        let mut delete_sub = client
            .queue_subscribe("mikrom.scheduler.delete_app", "schedulers".to_string())
            .await?;
        let mut delete_all_sub = client
            .queue_subscribe(
                "mikrom.scheduler.delete_all_by_app",
                "schedulers".to_string(),
            )
            .await?;
        let mut health_sub = client
            .queue_subscribe("mikrom.scheduler.check_health", "schedulers".to_string())
            .await?;
        let mut security_sub = client
            .queue_subscribe(
                "mikrom.scheduler.update_security_groups",
                "schedulers".to_string(),
            )
            .await?;

        tracing::info!("NATS Event Loop started, listening for messages...");

        loop {
            tokio::select! {
                Some(msg) = heartbeat_sub.next() => self.handle_heartbeat(msg).await,
                Some(msg) = router_heartbeat_sub.next() => self.handle_router_heartbeat(msg).await,
                Some(msg) = deploy_sub.next() => self.handle_deploy(msg).await,
                Some(msg) = status_sub.next() => self.handle_status(msg).await,
                Some(msg) = list_sub.next() => self.handle_list_apps(msg).await,
                Some(msg) = list_workers_sub.next() => self.handle_list_workers(msg).await,
                Some(msg) = pause_sub.next() => self.handle_pause(msg).await,
                Some(msg) = resume_sub.next() => self.handle_resume(msg).await,
                Some(msg) = cancel_sub.next() => self.handle_cancel(msg).await,
                Some(msg) = delete_sub.next() => self.handle_delete(msg).await,
                Some(msg) = delete_all_sub.next() => self.handle_delete_all(msg).await,
                Some(msg) = health_sub.next() => self.handle_check_health(msg).await,
                Some(msg) = security_sub.next() => self.handle_update_security_groups(msg).await,
            }
        }
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

    async fn handle_check_health(&self, message: async_nats::Message) {
        let server = self.server.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            if let Ok(req) =
                mikrom_proto::scheduler::CheckHealthRequest::decode(&message.payload[..])
            {
                let result = server.check_health(req).await;
                if let Some(reply) = message.reply {
                    let response = match result {
                        Ok(resp) => resp,
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
                    tracing::info!(
                        "Received heartbeat from router {} with WG IP {}",
                        heartbeat.host_id,
                        heartbeat.wireguard_ip
                    );
                    let worker = Worker {
                        host_id: heartbeat.host_id.clone(),
                        hostname: heartbeat.hostname.clone(),
                        ip_address: heartbeat.ip_address.clone(),
                        bridge_ip: "10.0.0.1/24".to_string(), // Dummy for router
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
                        // Broadcast mesh update immediately
                        if let Ok(workers) = server.app_service.worker_repo.list_workers().await {
                            // 1. Fetch all running jobs once and group by host_id
                            let mut jobs_by_host = std::collections::HashMap::new();
                            if let Ok(jobs) = server
                                .app_service
                                .job_repo
                                .list_jobs(None, None, Some(crate::domain::JobStatus::Running))
                                .await
                            {
                                for job in jobs {
                                    if let Some(host_id) = &job.host_id {
                                        jobs_by_host
                                            .entry(host_id.clone())
                                            .or_insert_with(Vec::new)
                                            .push(job);
                                    }
                                }
                            }

                            // 2. Build and broadcast update for each worker
                            for w in &workers {
                                let mut peers = Vec::new();
                                for peer_worker in &workers {
                                    // Skip self and only include peers with a public key
                                    if peer_worker.host_id == w.host_id
                                        || peer_worker.wireguard_pubkey.is_none()
                                    {
                                        continue;
                                    }

                                    let mut allowed_ips = Vec::new();
                                    if let Some(wg_ip) = &peer_worker.wireguard_ip
                                        && !wg_ip.is_empty()
                                    {
                                        let prefix =
                                            if wg_ip.contains(':') { "/128" } else { "/32" };
                                        allowed_ips.push(format!("{}{}", wg_ip, prefix));
                                    }

                                    // Use pre-grouped jobs
                                    if let Some(jobs) = jobs_by_host.get(&peer_worker.host_id) {
                                        for job in jobs {
                                            if let Some(ipv6) = &job.config.ipv6_address {
                                                let prefix =
                                                    if ipv6.contains(':') { "/128" } else { "/32" };
                                                allowed_ips.push(format!("{}{}", ipv6, prefix));
                                            }
                                        }
                                    }

                                    if !peer_worker
                                        .wireguard_pubkey
                                        .as_deref()
                                        .unwrap_or_default()
                                        .is_empty()
                                    {
                                        peers.push(mikrom_proto::scheduler::Peer {
                                            host_id: peer_worker.host_id.clone(),
                                            ip_address: peer_worker.ip_address.clone(),
                                            wireguard_pubkey: peer_worker
                                                .wireguard_pubkey
                                                .clone()
                                                .unwrap_or_default(),
                                            allowed_ips,
                                            wireguard_port: peer_worker
                                                .wireguard_port
                                                .unwrap_or(51820),
                                        });
                                    }
                                }

                                let update = mikrom_proto::scheduler::NetworkMeshUpdate { peers };
                                let mut buf = Vec::new();
                                if update.encode(&mut buf).is_ok() {
                                    let subject =
                                        format!("mikrom.scheduler.network.mesh.{}", w.host_id);
                                    let _ = client.publish(subject, buf.into()).await;
                                }
                            }
                        }
                    }
                },
                Err(e) => {
                    tracing::error!("Failed to decode router heartbeat: {}", e);
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
                    tracing::info!(
                        "Received heartbeat from worker {} with WG IP {}",
                        heartbeat.host_id,
                        heartbeat.wireguard_ip
                    );
                    let worker = Worker {
                        host_id: heartbeat.host_id.clone(),
                        hostname: heartbeat.hostname.clone(),
                        ip_address: heartbeat.ip_address.clone(),
                        bridge_ip: heartbeat.bridge_ip.clone(),
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
                        // Broadcast mesh update
                        if let Ok(workers) = server.app_service.worker_repo.list_workers().await {
                            for w in &workers {
                                let mut peers = Vec::new();
                                for peer_worker in &workers {
                                    // Skip self and only include peers with a public key
                                    if peer_worker.host_id == w.host_id
                                        || peer_worker.wireguard_pubkey.is_none()
                                    {
                                        continue;
                                    }

                                    let mut allowed_ips = Vec::new();
                                    if let Some(wg_ip) = &peer_worker.wireguard_ip
                                        && !wg_ip.is_empty()
                                    {
                                        let prefix =
                                            if wg_ip.contains(':') { "/128" } else { "/32" };
                                        allowed_ips.push(format!("{}{}", wg_ip, prefix));
                                    }

                                    if let Ok(jobs) = server
                                        .app_service
                                        .job_repo
                                        .list_jobs(
                                            None,
                                            None,
                                            Some(crate::domain::JobStatus::Running),
                                        )
                                        .await
                                    {
                                        for job in jobs {
                                            if job.host_id.as_deref() == Some(&peer_worker.host_id)
                                                && let Some(ipv6) = &job.config.ipv6_address
                                            {
                                                let prefix =
                                                    if ipv6.contains(':') { "/128" } else { "/32" };
                                                allowed_ips.push(format!("{}{}", ipv6, prefix));
                                            }
                                        }
                                    }

                                    if !peer_worker
                                        .wireguard_pubkey
                                        .as_deref()
                                        .unwrap_or_default()
                                        .is_empty()
                                    {
                                        peers.push(mikrom_proto::scheduler::Peer {
                                            host_id: peer_worker.host_id.clone(),
                                            ip_address: peer_worker.ip_address.clone(),
                                            wireguard_pubkey: peer_worker
                                                .wireguard_pubkey
                                                .clone()
                                                .unwrap_or_default(),
                                            allowed_ips,
                                            wireguard_port: peer_worker
                                                .wireguard_port
                                                .unwrap_or(51820),
                                        });
                                    }
                                }

                                let update = mikrom_proto::scheduler::NetworkMeshUpdate { peers };
                                let mut buf = Vec::new();
                                if update.encode(&mut buf).is_ok() {
                                    let subject =
                                        format!("mikrom.scheduler.network.mesh.{}", w.host_id);
                                    let _ = client.publish(subject, buf.into()).await;
                                }
                            }
                        }
                    }

                    if let Some(metrics) = heartbeat.metrics {
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
                let result = server.deploy_app(req).await;
                if let Some(reply) = message.reply {
                    let response = match result {
                        Ok(resp) => resp,
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
                let result = server.list_apps(req).await;
                if let Some(reply) = message.reply {
                    let response = match result {
                        Ok(resp) => resp,
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
                let result = server.pause_app(req).await;
                if let Some(reply) = message.reply {
                    let response = match result {
                        Ok(resp) => resp,
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
                let result = server.resume_app(req).await;
                if let Some(reply) = message.reply {
                    let response = match result {
                        Ok(resp) => resp,
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
                let result = server.delete_app(req).await;
                if let Some(reply) = message.reply {
                    let response = match result {
                        Ok(resp) => resp,
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
                let result = server.delete_all_by_app(req).await;
                if let Some(reply) = message.reply {
                    let response = match result {
                        Ok(resp) => resp,
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
}
