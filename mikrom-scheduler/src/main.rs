#![allow(clippy::collapsible_if)]

use mikrom_scheduler::config::SchedulerConfig;
use mikrom_scheduler::server::SchedulerServer;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = SchedulerConfig::load()?;

    mikrom_proto::telemetry::init_telemetry("mikrom-scheduler", env!("CARGO_PKG_VERSION"))?;

    tracing::info!("Connecting to database...");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&config.database_url)
        .await?;

    tracing::info!("Running database migrations...");
    sqlx::migrate!("./migrations").run(&pool).await?;

    let certs = if config.use_tls {
        tracing::info!("Loading TLS certificates from {}", config.certs_dir);
        Some(mikrom_proto::tls::ServiceCerts::load(&config.certs_dir)?)
    } else {
        None
    };

    tracing::info!("Connecting to NATS at {} for server...", config.nats_url);
    let nats_client = async_nats::connect(&config.nats_url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to NATS: {}", e))?;

    let server = SchedulerServer::new(pool.clone(), nats_client, certs)?;

    // Start NATS heartbeat listener
    let nats_url = config.nats_url.clone();
    let scheduler_for_nats = server.scheduler().clone();
    let server_for_nats = server.clone();

    // Combined NATS listener
    tokio::spawn(async move {
        loop {
            tracing::info!("Connecting to NATS at {} for listeners...", nats_url);
            match async_nats::connect(&nats_url).await {
                Ok(client) => {
                    tracing::info!("Connected to NATS for listeners");

                    // 1. Heartbeats (all schedulers subscribe to all heartbeats)
                    let mut heartbeat_subscription =
                        match client.subscribe("mikrom.scheduler.worker.heartbeat").await {
                            Ok(sub) => sub,
                            Err(e) => {
                                tracing::error!("Failed to subscribe to heartbeats: {}", e);
                                tokio::time::sleep(Duration::from_secs(5)).await;
                                continue;
                            },
                        };

                    // 2. Deployments (Queue Group for LB)
                    let mut deploy_subscription = match client
                        .queue_subscribe("mikrom.scheduler.deploy", "schedulers".to_string())
                        .await
                    {
                        Ok(sub) => sub,
                        Err(e) => {
                            tracing::error!("Failed to subscribe to deployments: {}", e);
                            return;
                        },
                    };

                    // 3. Status queries (Queue Group for LB)
                    let mut status_subscription = match client
                        .queue_subscribe("mikrom.scheduler.get_job", "schedulers".to_string())
                        .await
                    {
                        Ok(sub) => sub,
                        Err(e) => {
                            tracing::error!("Failed to subscribe to get_job: {}", e);
                            return;
                        },
                    };

                    let mut list_subscription = match client
                        .queue_subscribe("mikrom.scheduler.list_apps", "schedulers".to_string())
                        .await
                    {
                        Ok(sub) => sub,
                        Err(e) => {
                            tracing::error!("Failed to subscribe to list_apps: {}", e);
                            return;
                        },
                    };

                    let mut list_workers_subscription = match client
                        .queue_subscribe("mikrom.scheduler.list_workers", "schedulers".to_string())
                        .await
                    {
                        Ok(sub) => sub,
                        Err(e) => {
                            tracing::error!("Failed to subscribe to list_workers: {}", e);
                            return;
                        },
                    };

                    let mut pause_subscription = match client
                        .queue_subscribe("mikrom.scheduler.pause_app", "schedulers".to_string())
                        .await
                    {
                        Ok(sub) => sub,
                        Err(e) => {
                            tracing::error!("Failed to subscribe to pause_app: {}", e);
                            return;
                        },
                    };

                    let mut resume_subscription = match client
                        .queue_subscribe("mikrom.scheduler.resume_app", "schedulers".to_string())
                        .await
                    {
                        Ok(sub) => sub,
                        Err(e) => {
                            tracing::error!("Failed to subscribe to resume_app: {}", e);
                            return;
                        },
                    };

                    let mut cancel_subscription = match client
                        .queue_subscribe("mikrom.scheduler.cancel_app", "schedulers".to_string())
                        .await
                    {
                        Ok(sub) => sub,
                        Err(e) => {
                            tracing::error!("Failed to subscribe to cancel_app: {}", e);
                            return;
                        },
                    };

                    let mut delete_subscription = match client
                        .queue_subscribe("mikrom.scheduler.delete_app", "schedulers".to_string())
                        .await
                    {
                        Ok(sub) => sub,
                        Err(e) => {
                            tracing::error!("Failed to subscribe to delete_app: {}", e);
                            return;
                        },
                    };

                    use futures::StreamExt;
                    loop {
                        tokio::select! {
                            // Handle Heartbeats
                            Some(message) = heartbeat_subscription.next() => {
                                let worker_registry = scheduler_for_nats.worker_registry().clone();
                                tokio::spawn(async move {
                                    use prost::Message;
                                    use mikrom_proto::scheduler::WorkerHeartbeat;
                                    if let Ok(heartbeat) = WorkerHeartbeat::decode(&message.payload[..]) {
                                        let host_id = heartbeat.host_id;
                                        let hostname = heartbeat.hostname;
                                        let ip_address = heartbeat.ip_address;
                                        let agent_port = heartbeat.agent_port as u16;
                                        let bridge_ip = heartbeat.bridge_ip;

                                        tracing::info!("Received heartbeat from {} ({}) via Protobuf", hostname, host_id);

                                        if let Err(e) = worker_registry.register(host_id.clone(), hostname.clone(), ip_address, agent_port, bridge_ip).await {
                                            tracing::error!("Failed to register worker {} from NATS: {}", host_id, e);
                                        }

                                        if let Some(metrics) = heartbeat.metrics {
                                            let host_metrics = mikrom_scheduler::worker_registry::HostMetrics {
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
                                                vms: metrics.vms.into_iter().map(|(k, v)| {
                                                    (k, mikrom_scheduler::metrics::VmMetrics {
                                                        cpu_usage: v.cpu_usage,
                                                        ram_used_bytes: v.ram_used_bytes,
                                                    })
                                                }).collect(),
                                            };
                                            let _ = worker_registry.update_metrics(&host_id, host_metrics).await;
                                        }
                                    }
                                });
                            },
                            // Handle Deployments
                            Some(message) = deploy_subscription.next() => {
                                let server = server_for_nats.clone();
                                let client_clone = client.clone();
                                tokio::spawn(async move {
                                    use prost::Message;
                                    use mikrom_proto::scheduler::{DeployRequest, DeployResponse};
                                    if let Ok(req) = DeployRequest::decode(&message.payload[..]) {
                                        tracing::info!(app_id = %req.app_id, "Received deployment request via NATS (Protobuf)");

                                        let result = server.deploy_app(req).await;
                                        if let Some(reply) = message.reply {
                                            let response = match result {
                                                Ok(resp) => resp,
                                                Err(e) => DeployResponse {
                                                    message: e.to_string(),
                                                    ..Default::default()
                                                }
                                            };
                                            let mut buf = Vec::new();
                                            if response.encode(&mut buf).is_ok() {
                                                let _ = client_clone.publish(reply, buf.into()).await;
                                            }
                                        }
                                    }
                                });
                            },
                            // Handle Get Job Status
                            Some(message) = status_subscription.next() => {
                                let server = server_for_nats.clone();
                                let client_clone = client.clone();
                                tokio::spawn(async move {
                                    use prost::Message;
                                    use mikrom_proto::scheduler::AppStatusRequest;
                                    if let Ok(req) = AppStatusRequest::decode(&message.payload[..]) {
                                        let result = server.get_app_status(req).await;
                                        if let Some(reply) = message.reply {
                                            if let Ok(resp) = result {
                                                let mut buf = Vec::new();
                                                if resp.encode(&mut buf).is_ok() {
                                                    let _ = client_clone.publish(reply, buf.into()).await;
                                                }
                                            }
                                        }
                                    }
                                });
                            },
                            // Handle List Apps
                            Some(message) = list_subscription.next() => {
                                let server = server_for_nats.clone();
                                let client_clone = client.clone();
                                tokio::spawn(async move {
                                    use prost::Message;
                                    use mikrom_proto::scheduler::ListAppsRequest;
                                    if let Ok(req) = ListAppsRequest::decode(&message.payload[..]) {
                                        let result = server.list_apps(req).await;
                                        if let Some(reply) = message.reply {
                                            if let Ok(resp) = result {
                                                let mut buf = Vec::new();
                                                if resp.encode(&mut buf).is_ok() {
                                                    let _ = client_clone.publish(reply, buf.into()).await;
                                                }
                                            }
                                        }
                                    }
                                });
                            },
                            // Handle List Workers
                            Some(message) = list_workers_subscription.next() => {
                                let server = server_for_nats.clone();
                                let client_clone = client.clone();
                                tokio::spawn(async move {
                                    use prost::Message;
                                    use mikrom_proto::scheduler::ListWorkersRequest;
                                    if let Ok(req) = ListWorkersRequest::decode(&message.payload[..]) {
                                        let result = server.list_workers(req).await;
                                        if let Some(reply) = message.reply {
                                            if let Ok(resp) = result {
                                                let mut buf = Vec::new();
                                                if resp.encode(&mut buf).is_ok() {
                                                    let _ = client_clone.publish(reply, buf.into()).await;
                                                }
                                            }
                                        }
                                    }
                                });
                            },
                            // Handle Pause
                            Some(message) = pause_subscription.next() => {
                                let server = server_for_nats.clone();
                                let client_clone = client.clone();
                                tokio::spawn(async move {
                                    use prost::Message;
                                    use mikrom_proto::scheduler::PauseRequest;
                                    if let Ok(req) = PauseRequest::decode(&message.payload[..]) {
                                        let result = server.pause_app(req).await;
                                        if let Some(reply) = message.reply {
                                            if let Ok(resp) = result {
                                                let mut buf = Vec::new();
                                                if resp.encode(&mut buf).is_ok() {
                                                    let _ = client_clone.publish(reply, buf.into()).await;
                                                }
                                            }
                                        }
                                    }
                                });
                            },
                            // Handle Resume
                            Some(message) = resume_subscription.next() => {
                                let server = server_for_nats.clone();
                                let client_clone = client.clone();
                                tokio::spawn(async move {
                                    use prost::Message;
                                    use mikrom_proto::scheduler::ResumeRequest;
                                    if let Ok(req) = ResumeRequest::decode(&message.payload[..]) {
                                        let result = server.resume_app(req).await;
                                        if let Some(reply) = message.reply {
                                            if let Ok(resp) = result {
                                                let mut buf = Vec::new();
                                                if resp.encode(&mut buf).is_ok() {
                                                    let _ = client_clone.publish(reply, buf.into()).await;
                                                }
                                            }
                                        }
                                    }
                                });
                            },
                            // Handle Cancel
                            Some(message) = cancel_subscription.next() => {
                                let server = server_for_nats.clone();
                                let client_clone = client.clone();
                                tokio::spawn(async move {
                                    use prost::Message;
                                    use mikrom_proto::scheduler::CancelRequest;
                                    if let Ok(req) = CancelRequest::decode(&message.payload[..]) {
                                        let result = server.cancel_app(req).await;
                                        if let Some(reply) = message.reply {
                                            if let Ok(resp) = result {
                                                let mut buf = Vec::new();
                                                if resp.encode(&mut buf).is_ok() {
                                                    let _ = client_clone.publish(reply, buf.into()).await;
                                                }
                                            }
                                        }
                                    }
                                });
                            },
                            // Handle Delete
                            Some(message) = delete_subscription.next() => {
                                let server = server_for_nats.clone();
                                let client_clone = client.clone();
                                tokio::spawn(async move {
                                    use prost::Message;
                                    use mikrom_proto::scheduler::DeleteAppRequest;
                                    if let Ok(req) = DeleteAppRequest::decode(&message.payload[..]) {
                                        let result = server.delete_app(req).await;
                                        if let Some(reply) = message.reply {
                                            if let Ok(resp) = result {
                                                let mut buf = Vec::new();
                                                if resp.encode(&mut buf).is_ok() {
                                                    let _ = client_clone.publish(reply, buf.into()).await;
                                                }
                                            }
                                        }
                                    }
                                });
                            }
                        }
                    }
                },
                Err(e) => {
                    tracing::error!(
                        "Failed to connect to NATS for listeners: {}. Retrying in 5s...",
                        e
                    );
                    tokio::time::sleep(Duration::from_secs(5)).await;
                },
            }
        }
    });

    // Start background cleanup task
    let pool_clone = pool.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            let now = chrono::Utc::now().timestamp();
            let threshold = now - 60; // 60 seconds heartbeat timeout

            let result = sqlx::query(
                "UPDATE workers SET status = 'Offline' WHERE last_heartbeat < $1 AND status = 'Online'"
            )
            .bind(threshold)
            .execute(&pool_clone)
            .await;

            match result {
                Ok(r) => {
                    if r.rows_affected() > 0 {
                        tracing::info!("Marked {} stale workers as Offline", r.rows_affected());
                    }
                },
                Err(e) => tracing::error!("Failed to cleanup stale workers: {}", e),
            }
        }
    });

    // Keep the main process alive for the NATS listeners
    std::future::pending::<()>().await;

    Ok(())
}
