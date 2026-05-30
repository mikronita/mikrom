use crate::wireguard::WireGuardManager;
use crate::wireguard::error::NetworkError;
use crate::wireguard::keys::KeyManager;
use futures::StreamExt;
use mikrom_proto::scheduler::{NetworkMeshUpdate, WorkerHeartbeat};
use prost::Message;
use std::sync::Arc;
use tokio::time::Duration;
use tracing::{debug, error, info};

pub struct MeshOrchestrator {
    wg_manager: Arc<WireGuardManager>,
    nats_client: async_nats::Client,
}

impl MeshOrchestrator {
    pub fn new(wg_manager: Arc<WireGuardManager>, nats_client: async_nats::Client) -> Self {
        Self {
            wg_manager,
            nats_client,
        }
    }

    pub async fn run(
        &self,
        host_id: String,
        private_key: String,
        advertise_address: Option<String>,
    ) -> Result<(), NetworkError> {
        let pub_key = KeyManager::get_public_key(&private_key)?;
        let wg_ip = crate::wireguard::helpers::derive_host_ipv6(&host_id).to_string();
        let wg_port = i32::from(self.wg_manager.listen_port());

        let orchestrator_task = self.run_orchestrator(host_id.clone(), private_key);
        let heartbeat_task =
            self.run_heartbeat_loop(host_id, pub_key, wg_ip, wg_port, advertise_address);

        tokio::select! {
            res = orchestrator_task => res,
            res = heartbeat_task => res,
        }
    }

    async fn run_orchestrator(
        &self,
        host_id: String,
        private_key: String,
    ) -> Result<(), NetworkError> {
        let subject = format!("mikrom.scheduler.network.mesh.{}", host_id);

        let mut sub = self
            .nats_client
            .subscribe(subject.clone())
            .await
            .map_err(|e| {
                NetworkError::Internal(format!("Failed to subscribe to mesh updates: {e}"))
            })?;

        info!("Mesh Orchestrator listening on {}", subject);

        while let Some(msg) = sub.next().await {
            match NetworkMeshUpdate::decode(&msg.payload[..]) {
                Ok(update) => {
                    debug!("Received mesh update with {} peers", update.peers.len());
                    if let Err(e) = self
                        .wg_manager
                        .update_peers(&update.peers, &private_key, &host_id)
                        .await
                    {
                        error!("Failed to update WireGuard peers: {e}");
                    }
                },
                Err(e) => {
                    error!("Failed to decode NetworkMeshUpdate: {e}");
                },
            }
        }

        Ok(())
    }

    async fn run_heartbeat_loop(
        &self,
        host_id: String,
        pub_key: String,
        wg_ip: String,
        wg_port: i32,
        advertise_address: Option<String>,
    ) -> Result<(), NetworkError> {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        let subject = "mikrom.scheduler.worker.heartbeat";

        info!("Starting heartbeat loop on subject {}", subject);

        loop {
            interval.tick().await;

            let heartbeat = WorkerHeartbeat {
                host_id: host_id.clone(),
                hostname: host_id.clone(),
                wireguard_pubkey: pub_key.clone(),
                wireguard_ip: wg_ip.clone(),
                wireguard_port: wg_port,
                // These fields are required by scheduler but optional for network-only nodes
                metrics: None,
                advertise_address: advertise_address.clone().unwrap_or_default(),
                supported_hypervisors: Vec::new(),
            };

            let mut buf = Vec::new();
            if let Err(e) = heartbeat.encode(&mut buf) {
                error!("Failed to encode WorkerHeartbeat: {e}");
                continue;
            }

            if let Err(e) = self
                .nats_client
                .publish(subject.to_string(), buf.into())
                .await
            {
                error!("Failed to publish heartbeat: {e}");
            } else {
                debug!("Worker heartbeat published");
            }
        }
    }
}
