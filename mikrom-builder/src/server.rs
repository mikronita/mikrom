use dashmap::DashMap;
use futures::StreamExt;
use prost::Message;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use uuid::Uuid;

use mikrom_proto::builder::{
    BuildRequest, BuildResponse, BuildStatus, GetBuildStatusRequest, GetBuildStatusResponse,
};

use crate::builder::{AppBuilder, GitMetadata};

pub struct BuilderServer {
    builder: Arc<AppBuilder>,
    builds: Arc<DashMap<String, BuildInfo>>,
}

#[derive(Clone, Debug)]
struct BuildInfo {
    id: String,
    status: BuildStatus,
    image_tag: Option<String>,
    message: Option<String>,
    exposed_port: u32,
    git_commit_hash: Option<String>,
    git_commit_message: Option<String>,
    git_branch: Option<String>,
}

impl BuilderServer {
    pub fn new(builder: AppBuilder) -> Self {
        Self {
            builder: Arc::new(builder),
            builds: Arc::new(DashMap::new()),
        }
    }

    pub async fn listen(&self, nats_client: async_nats::Client) -> anyhow::Result<()> {
        info!("Starting BuilderServer listeners...");

        let build_task = self.start_build_worker(nats_client.clone());
        let status_task = self.start_status_worker(nats_client);

        let (r1, r2) = tokio::join!(build_task, status_task);

        if let Err(e) = r1 {
            error!("Build worker failed: {}", e);
        }
        if let Err(e) = r2 {
            error!("Status worker failed: {}", e);
        }

        Ok(())
    }

    async fn start_build_worker(&self, nats: async_nats::Client) -> anyhow::Result<()> {
        let mut subscription = nats
            .queue_subscribe("mikrom.builder.build", "builders".to_string())
            .await
            .map_err(|e| anyhow::anyhow!("Build subscription failed: {}", e))?;

        info!("Listening for build requests on mikrom.builder.build (Queue Group: builders)");

        while let Some(message) = subscription.next().await {
            let nats = nats.clone();
            let builder = self.builder.clone();
            let builds = self.builds.clone();

            tokio::spawn(async move {
                if let Err(e) = Self::handle_build_request(nats, builder, builds, message).await {
                    error!("Error handling build request: {}", e);
                }
            });
        }
        Ok(())
    }

    async fn handle_build_request(
        nats: async_nats::Client,
        builder: Arc<AppBuilder>,
        builds: Arc<DashMap<String, BuildInfo>>,
        message: async_nats::Message,
    ) -> anyhow::Result<()> {
        let req = BuildRequest::decode(&message.payload[..])
            .map_err(|e| anyhow::anyhow!("Failed to decode BuildRequest: {}", e))?;

        let build_id = Uuid::new_v4().to_string();
        info!(build_id = %build_id, app_id = %req.app_id, "Received build request");

        // Acknowledge build start if reply subject exists
        if let Some(reply) = message.reply {
            let resp = BuildResponse {
                success: true,
                build_id: build_id.clone(),
                message: "Build started".to_string(),
            };
            let _ = nats.publish(reply, resp.encode_to_vec().into()).await;
        }

        // Initialize build state
        builds.insert(
            build_id.clone(),
            BuildInfo {
                id: build_id.clone(),
                status: BuildStatus::Building,
                image_tag: None,
                message: None,
                exposed_port: 0,
                git_commit_hash: None,
                git_commit_message: None,
                git_branch: None,
            },
        );

        let (tx, rx) = mpsc::channel::<GitMetadata>(1);

        // Spawn metadata monitor
        tokio::spawn(Self::monitor_git_metadata(
            build_id.clone(),
            builds.clone(),
            nats.clone(),
            rx,
        ));

        // Execute build
        let result = builder
            .build_image(
                &req.app_id,
                &req.git_url,
                req.git_auth_token,
                &req.image_name,
                &req.tag,
                Some(tx),
            )
            .await;

        // Finalize state and notify
        Self::finalize_build(build_id, builds, nats, result).await;

        Ok(())
    }

    async fn monitor_git_metadata(
        build_id: String,
        builds: Arc<DashMap<String, BuildInfo>>,
        nats: async_nats::Client,
        mut rx: mpsc::Receiver<GitMetadata>,
    ) {
        if let Some(meta) = rx.recv().await
            && let Some(mut info) = builds.get_mut(&build_id)
        {
            info.git_commit_hash = Some(meta.hash.clone());
            info.git_commit_message = Some(meta.message.clone());
            info.git_branch = Some(meta.branch.clone());

            let progress = GetBuildStatusResponse {
                build_id: build_id.clone(),
                status: BuildStatus::Building as i32,
                git_commit_hash: meta.hash,
                git_commit_message: meta.message,
                git_branch: meta.branch,
                ..Default::default()
            };

            let _ = nats
                .publish(
                    format!("mikrom.builder.{}.status", build_id),
                    progress.encode_to_vec().into(),
                )
                .await;
        }
    }

    async fn finalize_build(
        build_id: String,
        builds: Arc<DashMap<String, BuildInfo>>,
        nats: async_nats::Client,
        result: anyhow::Result<crate::builder::BuildResult>,
    ) {
        let status_response = if let Some(mut info) = builds.get_mut(&build_id) {
            match result {
                Ok(res) => {
                    info!(build_id = %build_id, "Build successful");
                    info.status = BuildStatus::Success;
                    info.image_tag = Some(res.image_tag.clone());
                    info.exposed_port = res.exposed_port;

                    GetBuildStatusResponse {
                        build_id: build_id.clone(),
                        status: BuildStatus::Success as i32,
                        image_tag: res.image_tag,
                        exposed_port: res.exposed_port,
                        git_commit_hash: info.git_commit_hash.clone().unwrap_or_default(),
                        git_commit_message: info.git_commit_message.clone().unwrap_or_default(),
                        git_branch: info.git_branch.clone().unwrap_or_default(),
                        message: "Build successful".to_string(),
                    }
                },
                Err(e) => {
                    error!(build_id = %build_id, error = %e, "Build failed");
                    info.status = BuildStatus::Failed;
                    info.message = Some(e.to_string());

                    GetBuildStatusResponse {
                        build_id: build_id.clone(),
                        status: BuildStatus::Failed as i32,
                        message: e.to_string(),
                        ..Default::default()
                    }
                },
            }
        } else {
            warn!(build_id = %build_id, "Build record not found during finalization");
            return;
        };

        let _ = nats
            .publish(
                format!("mikrom.builder.{}.status", build_id),
                status_response.encode_to_vec().into(),
            )
            .await;
    }

    async fn start_status_worker(&self, nats: async_nats::Client) -> anyhow::Result<()> {
        let mut subscription = nats
            .queue_subscribe("mikrom.builder.get_status", "builders".to_string())
            .await
            .map_err(|e| anyhow::anyhow!("Status subscription failed: {}", e))?;

        info!("Listening for build status requests on mikrom.builder.get_status");

        while let Some(message) = subscription.next().await {
            let nats = nats.clone();
            let builds = self.builds.clone();

            tokio::spawn(async move {
                if let Err(e) = Self::handle_status_request(nats, builds, message).await {
                    error!("Error handling status request: {}", e);
                }
            });
        }
        Ok(())
    }

    async fn handle_status_request(
        nats: async_nats::Client,
        builds: Arc<DashMap<String, BuildInfo>>,
        message: async_nats::Message,
    ) -> anyhow::Result<()> {
        let reply = message
            .reply
            .ok_or_else(|| anyhow::anyhow!("Status request missing reply subject"))?;

        let req = GetBuildStatusRequest::decode(&message.payload[..])
            .map_err(|e| anyhow::anyhow!("Failed to decode GetBuildStatusRequest: {}", e))?;

        let resp = match builds.get(&req.build_id) {
            Some(info) => GetBuildStatusResponse {
                build_id: info.id.clone(),
                status: info.status as i32,
                image_tag: info.image_tag.clone().unwrap_or_default(),
                message: info.message.clone().unwrap_or_default(),
                exposed_port: info.exposed_port,
                git_commit_hash: info.git_commit_hash.clone().unwrap_or_default(),
                git_commit_message: info.git_commit_message.clone().unwrap_or_default(),
                git_branch: info.git_branch.clone().unwrap_or_default(),
            },
            None => GetBuildStatusResponse {
                build_id: req.build_id,
                status: BuildStatus::Failed as i32,
                message: "Build not found".to_string(),
                ..Default::default()
            },
        };

        nats.publish(reply, resp.encode_to_vec().into())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to publish status response: {}", e))?;

        Ok(())
    }
}
