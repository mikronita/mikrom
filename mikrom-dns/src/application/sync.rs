#![allow(
    clippy::cast_precision_loss,
    clippy::let_and_return,
    clippy::manual_let_else,
    clippy::missing_const_for_fn,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::non_std_lazy_statics,
    clippy::single_match_else,
    clippy::struct_field_names,
    clippy::suboptimal_flops,
    clippy::unchecked_time_subtraction,
    clippy::unused_async
)]

use crate::application::records::DnsRecordStore;
use crate::infrastructure::metrics;
use anyhow::Result;
use futures::StreamExt;
use mikrom_proto::scheduler::{AppInfo, DeployStatus, WorkerHeartbeat};
use prost::Message;
use std::net::Ipv6Addr;
use tracing::info;

pub struct DnsSyncService {
    store: DnsRecordStore,
}

impl DnsSyncService {
    pub fn new(store: DnsRecordStore) -> Self {
        Self { store }
    }

    pub fn handle_app_info(&self, info: AppInfo) -> bool {
        let key = format!("{}.{}", info.app_name, info.tenant_id);
        match DeployStatus::try_from(info.status) {
            Ok(DeployStatus::Running) => {
                if let Ok(ip) = info.ipv6_address.parse::<Ipv6Addr>() {
                    self.store.insert_user(key, ip);
                    return true;
                }
            },
            Ok(DeployStatus::Failed | DeployStatus::Cancelled | DeployStatus::Paused) => {
                self.store.remove_user(&key);
                return true;
            },
            _ => {},
        }

        false
    }

    pub fn handle_worker_heartbeat(&self, heartbeat: WorkerHeartbeat) -> bool {
        if let Ok(ip) = heartbeat.wireguard_ip.parse::<Ipv6Addr>() {
            self.store.insert_network(heartbeat.host_id, ip);
            return true;
        }

        false
    }
}

pub async fn run_nats_subscriber(store: DnsRecordStore) -> Result<()> {
    let nats_url =
        std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
    let sync_service = DnsSyncService::new(store.clone());
    let mut backoff = std::time::Duration::from_secs(1);

    loop {
        info!(%nats_url, "Connecting to NATS...");
        let client = match async_nats::connect(nats_url.clone()).await {
            Ok(client) => client,
            Err(err) => {
                tracing::warn!(error = %err, "Failed to connect to NATS, retrying");
                tokio::time::sleep(backoff).await;
                backoff = std::cmp::min(
                    backoff.saturating_mul(2),
                    std::time::Duration::from_secs(30),
                );
                continue;
            },
        };

        let job_updates_subject = mikrom_proto::subjects::SCHEDULER_JOB_UPDATES;
        let mut job_subscriber = match client.subscribe(job_updates_subject.to_string()).await {
            Ok(subscriber) => subscriber,
            Err(err) => {
                tracing::warn!(error = %err, "Failed to subscribe to job updates, retrying");
                tokio::time::sleep(backoff).await;
                backoff = std::cmp::min(
                    backoff.saturating_mul(2),
                    std::time::Duration::from_secs(30),
                );
                continue;
            },
        };

        let worker_heartbeat_subject = mikrom_proto::subjects::SCHEDULER_WORKER_HEARTBEAT;
        let mut worker_subscriber = match client
            .subscribe(worker_heartbeat_subject.to_string())
            .await
        {
            Ok(subscriber) => subscriber,
            Err(err) => {
                tracing::warn!(error = %err, "Failed to subscribe to worker heartbeats, retrying");
                tokio::time::sleep(backoff).await;
                backoff = std::cmp::min(
                    backoff.saturating_mul(2),
                    std::time::Duration::from_secs(30),
                );
                continue;
            },
        };

        backoff = std::time::Duration::from_secs(1);
        let reconnect = loop {
            tokio::select! {
                message = job_subscriber.next() => {
                    match message {
                        Some(message) => {
                            if let Ok(info) = AppInfo::decode(&message.payload[..]) && sync_service.handle_app_info(info) {
                                metrics::set_active_records(store.active_records());
                            }
                        }
                        None => {
                            break true;
                        }
                    }
                }
                message = worker_subscriber.next() => {
                    match message {
                        Some(message) => {
                            if let Ok(heartbeat) = WorkerHeartbeat::decode(&message.payload[..]) && sync_service.handle_worker_heartbeat(heartbeat) {
                                metrics::set_active_records(store.active_records());
                            }
                        }
                        None => {
                            break true;
                        }
                    }
                }
            }
        };

        if reconnect {
            tracing::warn!("NATS subscriber stream ended, reconnecting");
            tokio::time::sleep(backoff).await;
            backoff = std::cmp::min(
                backoff.saturating_mul(2),
                std::time::Duration::from_secs(30),
            );
        }
    }
}
