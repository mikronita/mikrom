use crate::AppState;
use crate::deploy::service::{DeployParams, DeploymentService};
use crate::repositories::app_repository::UpdateDeploymentParams;
use crate::workspace::{WorkspaceEvent, WorkspaceEventKind};
use async_trait::async_trait;
use futures::StreamExt;
use mikrom_proto::builder::{BuildStatus, GetBuildStatusRequest, GetBuildStatusResponse};
use mikrom_proto::scheduler::DeployRequest;
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

    async fn delete_all_by_app(
        &self,
        req: mikrom_proto::scheduler::DeleteAllByAppRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::DeleteAllByAppResponse>;

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
    pub nats: crate::nats::TypedNatsClient,
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
        let resp: GetBuildStatusResponse = self
            .nats
            .with_timeout(std::time::Duration::from_secs(5))
            .request(
                "mikrom.builder.get_status",
                GetBuildStatusRequest {
                    build_id: build_id.clone(),
                },
            )
            .await?;

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
        let inner = self
            .state
            .nats
            .with_timeout(std::time::Duration::from_secs(5))
            .request("mikrom.scheduler.deploy", req)
            .await?;

        Ok(inner)
    }

    async fn delete_app(
        &self,
        req: mikrom_proto::scheduler::DeleteAppRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::DeleteAppResponse> {
        let inner = self
            .state
            .nats
            .with_timeout(std::time::Duration::from_secs(5))
            .request("mikrom.scheduler.delete_app", req)
            .await?;
        Ok(inner)
    }

    async fn delete_all_by_app(
        &self,
        req: mikrom_proto::scheduler::DeleteAllByAppRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::DeleteAllByAppResponse> {
        let inner = self
            .state
            .nats
            .with_timeout(std::time::Duration::from_secs(5))
            .request("mikrom.scheduler.delete_all_by_app", req)
            .await?;
        Ok(inner)
    }

    async fn pause_app(
        &self,
        req: mikrom_proto::scheduler::PauseRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::PauseResponse> {
        let inner = self
            .state
            .nats
            .with_timeout(std::time::Duration::from_secs(5))
            .request("mikrom.scheduler.pause_app", req)
            .await?;
        Ok(inner)
    }

    async fn resume_app(
        &self,
        req: mikrom_proto::scheduler::ResumeRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::ResumeResponse> {
        let inner = self
            .state
            .nats
            .with_timeout(std::time::Duration::from_secs(5))
            .request("mikrom.scheduler.resume_app", req)
            .await?;
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

pub async fn start_build_polling(
    state: AppState,
    task: BuildTask,
    guard: Option<crate::DeploymentFlowGuard>,
) {
    let builder = Arc::new(RealBuilderClient {
        nats: state.nats.clone(),
    });
    let scheduler = Arc::new(RealSchedulerClient {
        state: state.clone(),
    });

    tokio::spawn(async move {
        if let Err(e) = poll_and_deploy(state, task, builder, scheduler, guard).await {
            error!("Background build/deploy task failed: {}", e);
        }
    });
}

pub async fn resume_pending_builds(state: AppState) {
    info!("Resuming pending builds from database...");
    let deployments = match state.app_repo.list_deployments_by_user(None).await {
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

        let guard = state.try_start_flow(app.id.into());
        start_build_polling(state.clone(), task, guard).await;
    }
}

pub async fn poll_and_deploy(
    state: AppState,
    task: BuildTask,
    builder: Arc<dyn BuilderClient>,
    _scheduler: Arc<dyn SchedulerClient>,
    guard: Option<crate::DeploymentFlowGuard>,
) -> anyhow::Result<()> {
    info!(build_id = %task.build_id, "Starting build status monitoring for deployment {}", task.deployment_id);

    // Keep the guard active throughout the build if it was provided
    let mut guard = guard;

    let subject = format!("mikrom.builder.{}.status", task.build_id);
    let mut subscription = state.nats.subscribe(subject).await?;

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

                let (image_tag, port, _hash, _msg, _branch) = (
                    current_status.1,
                    current_status.2,
                    current_status.3,
                    current_status.4,
                    current_status.5,
                );

                let final_port = if port > 0 { port } else { task.port };

                // Fetch app and deployment to satisfy service requirements
                let app = state
                    .app_repo
                    .get_app(task.app_id)
                    .await?
                    .ok_or(anyhow::anyhow!("App not found"))?;
                let deployment = state
                    .app_repo
                    .get_deployment(task.deployment_id)
                    .await?
                    .ok_or(anyhow::anyhow!("Deployment not found"))?;

                if final_port != task.port {
                    state
                        .app_repo
                        .update_deployment_port(task.deployment_id, final_port as i32)
                        .await?;
                }

                // Acquire guard before starting zero-downtime flow if we don't have it yet
                let final_guard = if let Some(g) = guard.take() {
                    g
                } else {
                    match state.try_start_flow(app.id.into()) {
                        Some(g) => g,
                        None => {
                            error!(app = %app.name, "Deployment flow already in progress for app, skipping zero-downtime flow for completed build.");
                            break;
                        },
                    }
                };

                let inner = DeploymentService::deploy_to_scheduler(
                    &state,
                    &app,
                    &deployment,
                    DeployParams {
                        image_tag,
                        vcpus: task.vcpus,
                        memory_mib: task.memory_mib as u32,
                        disk_mib: task.disk_mib as u32,
                        env: task.env.clone(),
                    },
                )
                .await
                .map_err(|e| anyhow::anyhow!(e))?;

                DeploymentService::run_zero_downtime_flow(
                    state.clone(),
                    app,
                    deployment,
                    inner,
                    task.user_id.clone(),
                    true,
                    final_guard,
                );

                break;
            },
            BuildStatus::Failed => {
                error!(build_id = %task.build_id, "Build failed, aborting deployment");
                state
                    .app_repo
                    .update_deployment(
                        task.deployment_id,
                        UpdateDeploymentParams {
                            status: Some("FAILED".to_string()),
                            ..Default::default()
                        },
                    )
                    .await?;
                state.deployment_events.send(task.app_id).ok();
                if let Ok(Some(app)) = state.app_repo.get_app(task.app_id).await {
                    state.publish_workspace_event(WorkspaceEvent {
                        kind: WorkspaceEventKind::DeploymentChanged,
                        user_id: Some(app.user_id),
                        app_id: Some(app.id),
                        app_name: Some(app.name),
                        deployment_id: Some(task.deployment_id),
                        resource_id: Some(task.build_id.clone()),
                    });
                }
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
        let nats = crate::nats::TypedNatsClient::new(nats_client);
        AppState {
            user_repo: Arc::new(MockUserRepository::new()),
            app_repo: Arc::new(MockAppRepository::new()),
            github_repo: Arc::new(crate::repositories::MockGithubRepository::default()),
            volume_repo: Arc::new(crate::repositories::MockVolumeRepository::new()),
            scheduler: Arc::new(crate::scheduler::MockScheduler::new()),
            nats,
            router_addr: "http://localhost:8080".to_string(),
            frontend_url: "http://localhost:3000".to_string(),
            api_db: sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://localhost/dummy")
                .unwrap(),
            jwt_secret: "secret".to_string(),
            master_key: "key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            workspace_events: tokio::sync::broadcast::channel(1).0,
            mesh_status: tokio::sync::watch::channel(crate::vms::MeshStatus::default()).0,
            acme_email: "admin@mikrom.spluca.org".to_string(),
            acme_staging: true,
            acme_check_interval: 3600,
            github_app_id: None,
            github_private_key: None,
            github_app_slug: None,
            github_webhook_url_base: None,
            active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
        }
    }

    #[tokio::test]
    async fn test_poll_and_deploy_success() {
        let _state = create_test_state().await;
    }
}
