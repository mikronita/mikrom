use crate::AppState;
use async_trait::async_trait;
use futures::StreamExt;
use mikrom_proto::builder::{BuildStatus, GetBuildStatusRequest, GetBuildStatusResponse};
use mikrom_proto::scheduler::{AppConfig, DeployRequest};
use prost::Message;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};
use uuid::Uuid;

#[async_trait]
pub trait BuilderClient: Send + Sync {
    async fn get_build_status(
        &self,
        build_id: String,
    ) -> anyhow::Result<(
        BuildStatus,
        String,
        u32,
        Option<String>,
        Option<String>,
        Option<String>,
    )>;
}

#[async_trait]
pub trait SchedulerClient: Send + Sync {
    async fn deploy_app(
        &self,
        req: DeployRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::DeployResponse>;

    async fn delete_app(
        &self,
        req: mikrom_proto::scheduler::DeleteAppRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::DeleteAppResponse>;

    async fn pause_app(
        &self,
        req: mikrom_proto::scheduler::PauseRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::PauseResponse>;

    async fn resume_app(
        &self,
        req: mikrom_proto::scheduler::ResumeRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::ResumeResponse>;
}

pub struct RealBuilderClient {
    pub nats_client: async_nats::Client,
}

#[async_trait]
impl BuilderClient for RealBuilderClient {
    async fn get_build_status(
        &self,
        build_id: String,
    ) -> anyhow::Result<(
        BuildStatus,
        String,
        u32,
        Option<String>,
        Option<String>,
        Option<String>,
    )> {
        let mut buf = Vec::new();
        GetBuildStatusRequest {
            build_id: build_id.clone(),
        }
        .encode(&mut buf)?;

        let response = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.nats_client
                .request("mikrom.builder.get_status", buf.into()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("NATS build status request timed out"))?
        .map_err(|e| anyhow::anyhow!("NATS build status request failed: {}", e))?;

        let resp = GetBuildStatusResponse::decode(&response.payload[..])?;

        let status = BuildStatus::try_from(resp.status).unwrap_or(BuildStatus::Unspecified);
        Ok((
            status,
            resp.image_tag,
            resp.exposed_port,
            if resp.git_commit_hash.is_empty() {
                None
            } else {
                Some(resp.git_commit_hash)
            },
            if resp.git_commit_message.is_empty() {
                None
            } else {
                Some(resp.git_commit_message)
            },
            if resp.git_branch.is_empty() {
                None
            } else {
                Some(resp.git_branch)
            },
        ))
    }
}

pub struct RealSchedulerClient {
    pub state: AppState,
}

#[async_trait]
impl SchedulerClient for RealSchedulerClient {
    async fn deploy_app(
        &self,
        req: DeployRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::DeployResponse> {
        let mut payload = Vec::new();
        req.encode(&mut payload)?;

        let response = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.state
                .nats_client
                .request("mikrom.scheduler.deploy", payload.into()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("NATS deployment request timed out"))?
        .map_err(|e| anyhow::anyhow!("NATS deployment failed: {}", e))?;

        let inner = mikrom_proto::scheduler::DeployResponse::decode(&response.payload[..])?;

        Ok(inner)
    }

    async fn delete_app(
        &self,
        req: mikrom_proto::scheduler::DeleteAppRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::DeleteAppResponse> {
        let mut payload = Vec::new();
        req.encode(&mut payload)?;
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.state
                .nats_client
                .request("mikrom.scheduler.delete_app", payload.into()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("NATS request timed out"))?
        .map_err(|e| anyhow::anyhow!("NATS request failed: {}", e))?;
        let inner = mikrom_proto::scheduler::DeleteAppResponse::decode(&response.payload[..])?;
        Ok(inner)
    }

    async fn pause_app(
        &self,
        req: mikrom_proto::scheduler::PauseRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::PauseResponse> {
        let mut payload = Vec::new();
        req.encode(&mut payload)?;
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.state
                .nats_client
                .request("mikrom.scheduler.pause_app", payload.into()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("NATS request timed out"))?
        .map_err(|e| anyhow::anyhow!("NATS request failed: {}", e))?;
        let inner = mikrom_proto::scheduler::PauseResponse::decode(&response.payload[..])?;
        Ok(inner)
    }

    async fn resume_app(
        &self,
        req: mikrom_proto::scheduler::ResumeRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::ResumeResponse> {
        let mut payload = Vec::new();
        req.encode(&mut payload)?;
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.state
                .nats_client
                .request("mikrom.scheduler.resume_app", payload.into()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("NATS request timed out"))?
        .map_err(|e| anyhow::anyhow!("NATS request failed: {}", e))?;
        let inner = mikrom_proto::scheduler::ResumeResponse::decode(&response.payload[..])?;
        Ok(inner)
    }
}

#[derive(Debug, Clone)]
pub struct BuildTask {
    pub deployment_id: Uuid,
    pub app_id: Uuid,
    pub build_id: String,
    pub app_name: String,
    pub user_id: String,
    pub vcpus: u32,
    pub memory_mib: u64,
    pub disk_mib: u64,
    pub port: u32,
    pub env: HashMap<String, String>,
}

pub async fn start_build_polling(state: AppState, task: BuildTask) {
    let builder = Arc::new(RealBuilderClient {
        nats_client: state.nats_client.clone(),
    });
    let scheduler = Arc::new(RealSchedulerClient {
        state: state.clone(),
    });

    tokio::spawn(async move {
        if let Err(e) = poll_and_deploy(state, task, builder, scheduler).await {
            error!("Background build/deploy task failed: {}", e);
        }
    });
}

pub async fn resume_pending_builds(state: AppState) {
    info!("Resuming pending builds from database...");
    let deployments = match state.app_repo.list_deployments_by_user("all").await {
        Ok(deps) => deps
            .into_iter()
            .filter(|d| d.status == "BUILDING" && d.build_id.is_some())
            .collect::<Vec<_>>(),
        Err(e) => {
            error!("Failed to list deployments for resume: {}", e);
            return;
        },
    };

    for dep in deployments {
        let build_id = dep.build_id.clone().unwrap();
        let app = match state.app_repo.get_app(dep.app_id).await {
            Ok(Some(a)) => a,
            _ => continue,
        };

        let task = BuildTask {
            deployment_id: dep.id,
            app_id: app.id,
            build_id,
            app_name: app.name,
            user_id: dep.user_id.to_string(),
            vcpus: dep.vcpus as u32,
            memory_mib: dep.memory_mib as u64,
            disk_mib: dep.disk_mib as u64,
            port: dep.port as u32,
            env: serde_json::from_value(dep.env_vars).unwrap_or_default(),
        };

        start_build_polling(state.clone(), task).await;
    }
}

async fn poll_and_deploy(
    state: AppState,
    task: BuildTask,
    builder: Arc<dyn BuilderClient>,
    scheduler: Arc<dyn SchedulerClient>,
) -> anyhow::Result<()> {
    info!(build_id = %task.build_id, "Starting build status monitoring for deployment {}", task.deployment_id);

    let subject = format!("mikrom.builder.{}.status", task.build_id);
    let mut subscription = state.nats_client.subscribe(subject).await?;

    // Initial check in case it's already done
    let mut current_status = match builder.get_build_status(task.build_id.clone()).await {
        Ok(s) => s,
        Err(e) => {
            warn!(
                "Failed initial build status check for {}: {}",
                task.build_id, e
            );
            (BuildStatus::Building, String::new(), 0, None, None, None)
        },
    };

    loop {
        match current_status.0 {
            BuildStatus::Success => {
                info!(build_id = %task.build_id, "Build successful, triggering deployment...");

                let (image_tag, port, hash, msg, branch) = (
                    current_status.1,
                    current_status.2,
                    current_status.3,
                    current_status.4,
                    current_status.5,
                );

                let final_port = if port > 0 { port } else { task.port };

                // Update DB with detected port only if it differs from default to avoid unnecessary writes
                if final_port != task.port {
                    state
                        .app_repo
                        .update_deployment_port(task.deployment_id, final_port as i32)
                        .await?;
                }
                state
                    .app_repo
                    .update_deployment_status(
                        task.deployment_id,
                        "SCHEDULED",
                        None,
                        Some(image_tag.clone()),
                        None,
                        None,
                        hash,
                        msg,
                        branch,
                    )
                    .await?;
                state.deployment_events.send(task.app_id).ok();

                let deploy_req = DeployRequest {
                    app_id: task.app_id.to_string(),
                    app_name: task.app_name.clone(),
                    image: image_tag,
                    user_id: task.user_id.clone(),
                    config: Some(AppConfig {
                        vcpus: task.vcpus,
                        memory_mib: task.memory_mib as u32,
                        disk_mib: task.disk_mib as u32,
                        port: final_port,
                        env: task.env.clone(),
                        ..Default::default()
                    }),
                    deployment_id: task.deployment_id.to_string(),
                };

                match scheduler.deploy_app(deploy_req).await {
                    Ok(resp) => {
                        if resp.job_id.is_empty() {
                            error!(message = %resp.message, "Scheduler returned success but job_id is empty. Error: {}", resp.message);
                            state
                                .app_repo
                                .update_deployment_status(
                                    task.deployment_id,
                                    "FAILED",
                                    None,
                                    None,
                                    None,
                                    None,
                                    None,
                                    None,
                                    None,
                                )
                                .await?;
                        } else {
                            info!(job_id = %resp.job_id, "Deployment successfully triggered by scheduler");
                            let db_status = crate::scheduler::status_name(resp.status);
                            state
                                .app_repo
                                .update_deployment_status(
                                    task.deployment_id,
                                    db_status,
                                    Some(resp.job_id),
                                    None,
                                    None,
                                    Some(resp.ip_address),
                                    None,
                                    None,
                                    None,
                                )
                                .await?;

                            // Promote this deployment to be the active one for the app
                            info!(app = %task.app_name, deployment_id = %task.deployment_id, "Promoting new deployment to active");
                            let _ = state
                                .app_repo
                                .set_active_deployment(task.app_id, task.deployment_id)
                                .await;

                            // Give the DB a moment to ensure the update is committed
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

                            // Notify router
                            let _ = state.notify_router(task.app_id).await;
                        }
                        state.deployment_events.send(task.app_id).ok();
                    },
                    Err(e) => {
                        error!("Scheduler failed to deploy app (gRPC/NATS error): {}", e);
                        state
                            .app_repo
                            .update_deployment_status(
                                task.deployment_id,
                                "FAILED",
                                None,
                                None,
                                None,
                                None,
                                None,
                                None,
                                None,
                            )
                            .await?;
                    },
                }
                break;
            },
            BuildStatus::Failed => {
                error!(build_id = %task.build_id, "Build failed, aborting deployment");
                state
                    .app_repo
                    .update_deployment_status(
                        task.deployment_id,
                        "FAILED",
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                    .await?;
                state.deployment_events.send(task.app_id).ok();
                break;
            },
            _ => {
                // Still building, wait for NATS update or timeout and poll
                tokio::select! {
                    msg = subscription.next() => {
                        if let Some(msg) = msg {
                            let resp_res = GetBuildStatusResponse::decode(&msg.payload[..]);
                            if let Ok(resp) = resp_res {
                                info!(build_id = %task.build_id, status = ?resp.status(), "Received status update from NATS");
                                current_status = (
                                    resp.status(),
                                    resp.image_tag,
                                    resp.exposed_port,
                                    Some(resp.git_commit_hash).filter(|s| !s.is_empty()),
                                    Some(resp.git_commit_message).filter(|s| !s.is_empty()),
                                    Some(resp.git_branch).filter(|s| !s.is_empty()),
                                );
                            }
                        }
                    },
                    _ = tokio::time::sleep(Duration::from_secs(2)) => {
                        info!(build_id = %task.build_id, "Polling build status (fallback)...");
                        if let Ok(s) = builder.get_build_status(task.build_id.clone()).await {
                            current_status = s;
                        }
                    }
                }
            },
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::app_repository::MockAppRepository;
    use crate::repositories::user_repository::MockUserRepository;

    async fn create_test_state() -> AppState {
        let nats_url =
            std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
        let nats_client = async_nats::connect(nats_url).await.unwrap();
        AppState {
            user_repo: Arc::new(MockUserRepository::new()),
            app_repo: Arc::new(MockAppRepository::new()),
            scheduler: Arc::new(crate::scheduler::MockScheduler::new()),
            nats_client,
            router_addr: "http://localhost:8080".to_string(),
            jwt_secret: "secret".to_string(),
            master_key: "key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            build_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }

    #[tokio::test]
    async fn test_poll_and_deploy_success() {
        let _state = create_test_state().await;
    }
}
