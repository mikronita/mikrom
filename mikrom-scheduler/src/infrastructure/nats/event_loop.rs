use crate::domain::HostMetrics;
use crate::server::SchedulerServer;
use futures::StreamExt;
use mikrom_proto::scheduler::{
    AppStatusRequest, CancelRequest, DeleteAppRequest, DeployRequest, ListAppsRequest,
    ListWorkersRequest, PauseRequest, ResumeRequest, WorkerHeartbeat,
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

        tracing::info!("NATS Event Loop started, listening for messages...");

        loop {
            tokio::select! {
                Some(msg) = heartbeat_sub.next() => self.handle_heartbeat(msg).await,
                Some(msg) = deploy_sub.next() => self.handle_deploy(msg).await,
                Some(msg) = status_sub.next() => self.handle_status(msg).await,
                Some(msg) = list_sub.next() => self.handle_list_apps(msg).await,
                Some(msg) = list_workers_sub.next() => self.handle_list_workers(msg).await,
                Some(msg) = pause_sub.next() => self.handle_pause(msg).await,
                Some(msg) = resume_sub.next() => self.handle_resume(msg).await,
                Some(msg) = cancel_sub.next() => self.handle_cancel(msg).await,
                Some(msg) = delete_sub.next() => self.handle_delete(msg).await,
            }
        }
    }

    async fn handle_heartbeat(&self, message: async_nats::Message) {
        let server = self.server.clone();
        tokio::spawn(async move {
            if let Ok(heartbeat) = WorkerHeartbeat::decode(&message.payload[..]) {
                tracing::info!("Received heartbeat from worker {}", heartbeat.host_id);
                let worker = crate::domain::Worker {
                    host_id: heartbeat.host_id.clone(),
                    hostname: heartbeat.hostname.clone(),
                    ip_address: heartbeat.ip_address.clone(),
                    bridge_ip: heartbeat.bridge_ip.clone(),
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
}
