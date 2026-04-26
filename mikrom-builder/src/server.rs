use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tonic::{Request, Response, Status};
use tracing::{error, info};
use uuid::Uuid;

use mikrom_proto::builder::builder_service_server::BuilderService;
use mikrom_proto::builder::{
    BuildRequest, BuildResponse, BuildStatus, GetBuildStatusRequest, GetBuildStatusResponse,
};

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
}

#[tonic::async_trait]
impl BuilderService for BuilderServer {
    async fn build_app(
        &self,
        request: Request<BuildRequest>,
    ) -> Result<Response<BuildResponse>, Status> {
        let req = request.into_inner();
        let build_id = Uuid::new_v4().to_string();

        info!(
            build_id = %build_id,
            app_id = %req.app_id,
            git_url = %req.git_url,
            "Received build request"
        );

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

        self.builds.write().insert(build_id.clone(), build_info);

        let builder = self.builder.clone();
        let builds = self.builds.clone();
        let build_id_clone = build_id.clone();

        let (tx, mut rx) = tokio::sync::mpsc::channel::<crate::builder::GitMetadata>(1);

        // Spawn background task for building
        tokio::spawn(async move {
            // Task to listen for early metadata
            let builds_meta = builds.clone();
            let build_id_meta = build_id_clone.clone();
            tokio::spawn(async move {
                if let Some(meta) = rx.recv().await {
                    let mut lock = builds_meta.write();
                    if let Some(info) = lock.get_mut(&build_id_meta) {
                        info.git_commit_hash = Some(meta.hash);
                        info.git_commit_message = Some(meta.message);
                        info.git_branch = Some(meta.branch);
                        info!("Updated build info with early git metadata");
                    }
                }
            });

            match builder
                .build_image(
                    &req.app_id,
                    &req.git_url,
                    &req.image_name,
                    &req.tag,
                    Some(tx),
                )
                .await
            {
                Ok(result) => {
                    let mut lock = builds.write();
                    if let Some(info) = lock.get_mut(&build_id_clone) {
                        info.status = BuildStatus::Success;
                        info.image_tag = Some(result.image_tag);
                        info.exposed_port = result.exposed_port;
                        info.git_commit_hash = Some(result.git_commit_hash);
                        info.git_commit_message = Some(result.git_commit_message);
                        info.git_branch = Some(result.git_branch);
                    }
                },
                Err(e) => {
                    error!(build_id = %build_id_clone, error = %e, "Build failed");
                    let mut lock = builds.write();
                    if let Some(info) = lock.get_mut(&build_id_clone) {
                        info.status = BuildStatus::Failed;
                        info.message = Some(e.to_string());
                    }
                },
            }
        });

        Ok(Response::new(BuildResponse {
            success: true,
            build_id: build_id.clone(),
            message: "Build started".to_string(),
        }))
    }

    async fn get_build_status(
        &self,
        request: Request<GetBuildStatusRequest>,
    ) -> Result<Response<GetBuildStatusResponse>, Status> {
        let req = request.into_inner();
        let builds = self.builds.read();

        match builds.get(&req.build_id) {
            Some(info) => Ok(Response::new(GetBuildStatusResponse {
                build_id: info.id.clone(),
                status: info.status as i32,
                image_tag: info.image_tag.clone().unwrap_or_default(),
                message: info.message.clone().unwrap_or_default(),
                exposed_port: info.exposed_port,
                git_commit_hash: info.git_commit_hash.clone().unwrap_or_default(),
                git_commit_message: info.git_commit_message.clone().unwrap_or_default(),
                git_branch: info.git_branch.clone().unwrap_or_default(),
            })),
            None => Err(Status::not_found("Build not found")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mikrom_proto::builder::BuildStatus;

    #[tokio::test]
    async fn test_build_request_status_flow() {
        let builder = AppBuilder::new(
            "localhost:5000".into(),
            "paketobuildpacks/builder-jammy-base".into(),
        );
        let server = BuilderServer::new(builder);

        let req = Request::new(BuildRequest {
            app_id: "app-1".into(),
            git_url: "http://invalid".into(),
            image_name: "test".into(),
            tag: "v1".into(),
        });

        let resp = server.build_app(req).await.unwrap().into_inner();
        assert!(resp.success);
        let build_id = resp.build_id;

        // Check status immediately
        let status_req = Request::new(GetBuildStatusRequest {
            build_id: build_id.clone(),
        });
        let status_resp = server
            .get_build_status(status_req)
            .await
            .unwrap()
            .into_inner();
        assert_eq!(status_resp.status, BuildStatus::Building as i32);

        // Wait a bit for the background task to fail (since URL is invalid)
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let status_req = Request::new(GetBuildStatusRequest {
            build_id: build_id.clone(),
        });
        let status_resp = server
            .get_build_status(status_req)
            .await
            .unwrap()
            .into_inner();
        assert_eq!(status_resp.status, BuildStatus::Failed as i32);
        assert!(!status_resp.message.is_empty());
    }
}
