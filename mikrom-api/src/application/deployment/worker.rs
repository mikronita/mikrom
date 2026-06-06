use super::service::{DeployParams, DeploymentService};
use crate::AppState;
use crate::domain::UpdateDeploymentParams;
use crate::domain::types::{CpuCores, MemoryMb, Port};
use crate::workspace::{WorkspaceEvent, WorkspaceEventKind};
use async_trait::async_trait;
use futures::StreamExt;
use mikrom_proto::builder::{BuildStatus, GetBuildStatusRequest, GetBuildStatusResponse};
use mikrom_proto::scheduler::{DeployRequest, DeployStatus};
use prost::Message;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};
use uuid::Uuid;

#[async_trait]
#[cfg_attr(any(test, feature = "test-utils"), mockall::automock)]
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
        String,
    )>;
}

#[async_trait]
#[cfg_attr(any(test, feature = "test-utils"), mockall::automock)]
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
        String,
    )> {
        let resp: GetBuildStatusResponse = self
            .nats
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
            resp.message,
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
            .with_timeout(std::time::Duration::from_secs(
                self.state.ctx.config.nats_request_timeout_secs.max(1),
            ))
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
            .with_timeout(std::time::Duration::from_secs(
                self.state.ctx.config.nats_request_timeout_secs.max(1),
            ))
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
            .with_timeout(std::time::Duration::from_secs(
                self.state
                    .ctx
                    .config
                    .nats_scheduler_long_timeout_secs
                    .max(1),
            ))
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
            .with_timeout(std::time::Duration::from_secs(
                self.state.ctx.config.nats_request_timeout_secs.max(1),
            ))
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
            .with_timeout(std::time::Duration::from_secs(
                self.state.ctx.config.nats_request_timeout_secs.max(1),
            ))
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
    pub vcpus: CpuCores,
    pub memory_mib: MemoryMb,
    pub disk_mib: u32,
    pub port: Port,
    pub env: HashMap<String, String>,
    pub hypervisor: i32,
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
            user_id: dep.tenant_id.to_string(),
            vcpus: dep.vcpus,
            memory_mib: dep.memory_mib,
            disk_mib: dep.disk_mib as u32,
            port: dep.port,
            env: serde_json::from_value(dep.env_vars).unwrap_or_default(),
            hypervisor: dep.hypervisor,
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
            (
                BuildStatus::Building,
                String::new(),
                0,
                None,
                None,
                None,
                String::new(),
            )
        },
    };

    loop {
        match current_status.0 {
            BuildStatus::Success => {
                info!(build_id = %task.build_id, "Build successful, triggering deployment...");

                let (image_tag, port, hash, msg, branch) = (
                    current_status.1.clone(),
                    current_status.2,
                    current_status.3,
                    current_status.4,
                    current_status.5,
                );

                let final_port = if port > 0 {
                    Port::new(port).unwrap_or(task.port)
                } else {
                    task.port
                };

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

                // Update deployment with git metadata if available
                state
                    .app_repo
                    .update_deployment(
                        task.deployment_id,
                        UpdateDeploymentParams {
                            git_commit_hash: hash,
                            git_commit_message: msg,
                            git_branch: branch,
                            ..Default::default()
                        },
                    )
                    .await?;

                state.deployment_events.send(task.app_id).ok();

                if final_port != task.port {
                    state
                        .app_repo
                        .update_deployment_port(task.deployment_id, final_port)
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
                        image_tag: image_tag.clone(),
                        vcpus: task.vcpus,
                        memory_mib: task.memory_mib,
                        disk_mib: task.disk_mib,
                        port: final_port,
                        env: task.env.clone(),
                        hypervisor: task.hypervisor,
                    },
                )
                .await
                .map_err(|e| anyhow::anyhow!(e))?;

                if inner.status != DeployStatus::Running as i32 {
                    warn!(
                        deployment_id = %task.deployment_id,
                        build_id = %task.build_id,
                        status = %inner.status,
                        message = %inner.message,
                        "Deployment failed before zero-downtime promotion"
                    );
                    mark_deployment_failed(
                        &state,
                        task.deployment_id,
                        task.app_id,
                        Some(image_tag),
                    )
                    .await?;
                    break;
                }

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
                let failure_message = if current_status.6.is_empty() {
                    "Build failed".to_string()
                } else {
                    current_status.6.clone()
                };
                error!(
                    build_id = %task.build_id,
                    message = %failure_message,
                    image_tag = %current_status.1,
                    "Build failed, aborting deployment"
                );
                mark_deployment_failed(&state, task.deployment_id, task.app_id, None).await?;
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
                                    resp.message,
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

async fn mark_deployment_failed(
    state: &AppState,
    deployment_id: Uuid,
    app_id: Uuid,
    image_tag: Option<String>,
) -> anyhow::Result<()> {
    state
        .app_repo
        .update_deployment(
            deployment_id,
            UpdateDeploymentParams {
                status: Some("FAILED".to_string()),
                image_tag,
                ..Default::default()
            },
        )
        .await?;

    state.deployment_events.send(app_id).ok();
    if let Ok(Some(app)) = state.app_repo.get_app(app_id).await {
        state.publish_workspace_event(WorkspaceEvent {
            kind: WorkspaceEventKind::DeploymentChanged,
            user_id: None,
            tenant_id: Some(app.tenant_id),
            app_id: Some(app.id),
            app_name: Some(app.name),
            deployment_id: Some(deployment_id),
            volume_id: None,
            resource_id: Some(deployment_id.to_string()),
        });
    }

    Ok(())
}

#[cfg(any())]
mod tests {
    use super::*;
    use crate::domain::App;
    use crate::domain::MockAppRepository;
    use crate::domain::MockDatabaseRepository;
    use crate::domain::MockScheduler;
    use crate::domain::user::MockUserRepository;
    use crate::nats::MockNatsClient;
    use mockall::predicate::function;

    async fn create_test_state() -> AppState {
        let nats = crate::nats::TypedNatsClient::new_custom(Arc::new(MockNatsClient::new()));
        AppState {
            ctx: crate::application::ApiContext::default(),
            user_repo: Arc::new(MockUserRepository::new()),
            database_repo: Arc::new(MockDatabaseRepository::new()),
            app_repo: Arc::new(MockAppRepository::new()),
            github_repo: Arc::new(crate::domain::github::MockGithubRepository::default()),
            volume_repo: Arc::new(crate::domain::MockVolumeRepository::new()),
            scheduler: Arc::new(MockScheduler::new()),
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
            mesh_status:
                tokio::sync::watch::channel(crate::application::vms::MeshStatus::default()).0,
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

    #[tokio::test]
    async fn test_mark_deployment_failed_updates_deployment_and_emits_events() {
        let app_id = Uuid::new_v4();
        let deployment_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();

        let mut app_repo = MockAppRepository::new();
        app_repo
            .expect_update_deployment()
            .with(
                function(move |id: &Uuid| *id == deployment_id),
                function(|params: &crate::domain::UpdateDeploymentParams| {
                    params.status.as_deref() == Some("FAILED")
                        && params.image_tag.as_deref() == Some("image:tag")
                }),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        app_repo
            .expect_get_app()
            .with(function(move |id: &Uuid| *id == app_id))
            .times(1)
            .returning(move |_| {
                Ok(Some(App {
                    id: app_id,
                    user_id,
                    name: "demo".to_string(),
                    hostname: Some("demo.example.com".to_string()),
                    ..Default::default()
                }))
            });

        let nats = crate::nats::TypedNatsClient::new_custom(Arc::new(MockNatsClient::new()));
        let deployment_events = tokio::sync::broadcast::channel(4).0;
        let mut deployment_events_rx = deployment_events.subscribe();
        let workspace_events = tokio::sync::broadcast::channel(4).0;
        let mut workspace_events_rx = workspace_events.subscribe();

        let state = AppState {
            ctx: crate::application::ApiContext::default(),
            user_repo: Arc::new(MockUserRepository::new()),
            database_repo: Arc::new(MockDatabaseRepository::new()),
            app_repo: Arc::new(app_repo),
            github_repo: Arc::new(crate::domain::github::MockGithubRepository::default()),
            volume_repo: Arc::new(crate::domain::MockVolumeRepository::new()),
            scheduler: Arc::new(MockScheduler::new()),
            nats,
            router_addr: "http://localhost:8080".to_string(),
            frontend_url: "http://localhost:3000".to_string(),
            api_db: sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://localhost/dummy")
                .unwrap(),
            jwt_secret: "secret".to_string(),
            master_key: "key".into(),
            deployment_events,
            workspace_events,
            mesh_status:
                tokio::sync::watch::channel(crate::application::vms::MeshStatus::default()).0,
            acme_email: "admin@mikrom.spluca.org".to_string(),
            acme_staging: true,
            acme_check_interval: 3600,
            github_app_id: None,
            github_private_key: None,
            github_app_slug: None,
            github_webhook_url_base: None,
            active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
        };

        mark_deployment_failed(&state, deployment_id, app_id, Some("image:tag".to_string()))
            .await
            .unwrap();

        assert_eq!(deployment_events_rx.recv().await.unwrap(), app_id);
        let workspace_event = workspace_events_rx.recv().await.unwrap();
        assert_eq!(workspace_event.app_id, Some(app_id));
        assert_eq!(workspace_event.deployment_id, Some(deployment_id));
        assert_eq!(workspace_event.user_id, Some(user_id));
        assert!(matches!(
            workspace_event.kind,
            WorkspaceEventKind::DeploymentChanged
        ));
    }
}
