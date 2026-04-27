use futures::StreamExt;
use parking_lot::RwLock;
use prost::Message;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

use mikrom_proto::builder::{BuildRequest, BuildResponse, BuildStatus, GetBuildStatusResponse};

use crate::builder::AppBuilder;

pub struct BuilderServer {
    builder: Arc<AppBuilder>,
    builds: Arc<RwLock<HashMap<String, BuildInfo>>>,
}

#[derive(Clone)]
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
            builds: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn listen(&self, nats_client: async_nats::Client) -> anyhow::Result<()> {
        let nats_clone1 = nats_client.clone();
        let builder_clone = self.builder.clone();
        let builds_clone1 = self.builds.clone();

        // 1. Task for handling Build Requests
        let build_task = tokio::spawn(async move {
            let mut subscription = nats_clone1
                .queue_subscribe("mikrom.builder.build", "builders".to_string())
                .await
                .expect("Failed to subscribe to build topic");

            info!("Listening for build requests on mikrom.builder.build (Queue Group: builders)");

            while let Some(message) = subscription.next().await {
                let nats = nats_clone1.clone();
                let builder = builder_clone.clone();
                let builds = builds_clone1.clone();

                tokio::spawn(async move {
                    if let Ok(req) = BuildRequest::decode(&message.payload[..]) {
                        let build_id = Uuid::new_v4().to_string();
                        info!(build_id = %build_id, app_id = %req.app_id, "Received build request");

                        if let Some(reply) = message.reply {
                            let resp = BuildResponse {
                                success: true,
                                build_id: build_id.clone(),
                                message: "Build started".to_string(),
                            };
                            let _ = nats.publish(reply, resp.encode_to_vec().into()).await;
                        }

                        let build_info = BuildInfo {
                            id: build_id.clone(),
                            status: BuildStatus::Building,
                            image_tag: None,
                            message: None,
                            exposed_port: 0,
                            git_commit_hash: None,
                            git_commit_message: None,
                            git_branch: None,
                        };
                        builds.write().insert(build_id.clone(), build_info);

                        let (tx, mut rx) =
                            tokio::sync::mpsc::channel::<crate::builder::GitMetadata>(1);

                        // Metadata monitor task
                        let builds_meta = builds.clone();
                        let build_id_meta = build_id.clone();
                        let nats_meta = nats.clone();
                        tokio::spawn(async move {
                            if let Some(meta) = rx.recv().await {
                                let payload = {
                                    let mut lock = builds_meta.write();
                                    if let Some(info) = lock.get_mut(&build_id_meta) {
                                        info.git_commit_hash = Some(meta.hash.clone());
                                        info.git_commit_message = Some(meta.message.clone());
                                        info.git_branch = Some(meta.branch.clone());

                                        let progress = GetBuildStatusResponse {
                                            build_id: build_id_meta.clone(),
                                            status: BuildStatus::Building as i32,
                                            git_commit_hash: meta.hash,
                                            git_commit_message: meta.message,
                                            git_branch: meta.branch,
                                            ..Default::default()
                                        };
                                        Some(progress.encode_to_vec())
                                    } else {
                                        None
                                    }
                                };

                                if let Some(p) = payload {
                                    let _ = nats_meta
                                        .publish(
                                            format!("mikrom.builder.{}.status", build_id_meta),
                                            p.into(),
                                        )
                                        .await;
                                }
                            }
                        });

                        // Start build
                        let result = builder
                            .build_image(
                                &req.app_id,
                                &req.git_url,
                                &req.image_name,
                                &req.tag,
                                Some(tx),
                            )
                            .await;

                        let final_payload = {
                            let mut lock = builds.write();
                            if let Some(info) = lock.get_mut(&build_id) {
                                match result {
                                    Ok(res) => {
                                        info!(build_id = %build_id, "Build successful");
                                        info.status = BuildStatus::Success;
                                        info.image_tag = Some(res.image_tag.clone());
                                        info.exposed_port = res.exposed_port;
                                        let resp = GetBuildStatusResponse {
                                            build_id: build_id.clone(),
                                            status: BuildStatus::Success as i32,
                                            image_tag: res.image_tag,
                                            exposed_port: res.exposed_port,
                                            git_commit_hash: info
                                                .git_commit_hash
                                                .clone()
                                                .unwrap_or_default(),
                                            git_commit_message: info
                                                .git_commit_message
                                                .clone()
                                                .unwrap_or_default(),
                                            git_branch: info.git_branch.clone().unwrap_or_default(),
                                            message: "Build successful".to_string(),
                                        };
                                        Some(resp.encode_to_vec())
                                    },
                                    Err(e) => {
                                        error!(build_id = %build_id, error = %e, "Build failed");
                                        info.status = BuildStatus::Failed;
                                        info.message = Some(e.to_string());
                                        let resp = GetBuildStatusResponse {
                                            build_id: build_id.clone(),
                                            status: BuildStatus::Failed as i32,
                                            message: e.to_string(),
                                            ..Default::default()
                                        };
                                        Some(resp.encode_to_vec())
                                    },
                                }
                            } else {
                                None
                            }
                        };

                        if let Some(p) = final_payload {
                            let _ = nats
                                .publish(format!("mikrom.builder.{}.status", build_id), p.into())
                                .await;
                        }
                    }
                });
            }
        });

        // 2. Task for handling Status Requests
        let nats_clone2 = nats_client.clone();
        let builds_clone2 = self.builds.clone();
        let status_task = tokio::spawn(async move {
            let mut subscription = nats_clone2
                .queue_subscribe("mikrom.builder.get_status", "builders".to_string())
                .await
                .expect("Failed to subscribe to status topic");

            info!("Listening for build status requests on mikrom.builder.get_status");

            while let Some(message) = subscription.next().await {
                if let (Ok(req), Some(reply)) = (
                    mikrom_proto::builder::GetBuildStatusRequest::decode(&message.payload[..]),
                    message.reply,
                ) {
                    let resp_vec = {
                        let builds = builds_clone2.read();
                        let resp = match builds.get(&req.build_id) {
                            Some(info) => GetBuildStatusResponse {
                                build_id: info.id.clone(),
                                status: info.status as i32,
                                image_tag: info.image_tag.clone().unwrap_or_default(),
                                message: info.message.clone().unwrap_or_default(),
                                exposed_port: info.exposed_port,
                                git_commit_hash: info.git_commit_hash.clone().unwrap_or_default(),
                                git_commit_message: info
                                    .git_commit_message
                                    .clone()
                                    .unwrap_or_default(),
                                git_branch: info.git_branch.clone().unwrap_or_default(),
                            },
                            None => GetBuildStatusResponse {
                                build_id: req.build_id,
                                status: BuildStatus::Failed as i32,
                                message: "Build not found".to_string(),
                                ..Default::default()
                            },
                        };
                        resp.encode_to_vec()
                    };
                    let _ = nats_clone2.publish(reply, resp_vec.into()).await;
                }
            }
        });

        // Wait for both tasks
        let (r1, r2) = tokio::join!(build_task, status_task);
        if let Err(e) = r1 {
            error!("Build task failed: {}", e);
        }
        if let Err(e) = r2 {
            error!("Status task failed: {}", e);
        }

        Ok(())
    }
}
