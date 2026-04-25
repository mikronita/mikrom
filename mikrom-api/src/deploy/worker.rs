use crate::AppState;
use async_trait::async_trait;
use mikrom_proto::builder::{BuildStatus, BuilderServiceClient, GetBuildStatusRequest};
use mikrom_proto::scheduler::{AppConfig, DeployRequest};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

#[async_trait]
pub trait BuilderClient: Send + Sync {
    async fn get_build_status(
        &self,
        build_id: String,
    ) -> anyhow::Result<(BuildStatus, String, u32)>;
}

#[async_trait]
pub trait SchedulerClient: Send + Sync {
    async fn deploy_app(
        &self,
        req: DeployRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::DeployResponse>;
}

pub struct RealBuilderClient {
    pub addr: String,
}

#[async_trait]
impl BuilderClient for RealBuilderClient {
    async fn get_build_status(
        &self,
        build_id: String,
    ) -> anyhow::Result<(BuildStatus, String, u32)> {
        let channel = crate::builder::connect(&self.addr).await?;
        let mut client = BuilderServiceClient::new(channel);
        let resp = client
            .get_build_status(GetBuildStatusRequest { build_id })
            .await?
            .into_inner();
        let status = BuildStatus::try_from(resp.status).unwrap_or(BuildStatus::Unspecified);
        Ok((status, resp.image_tag, resp.exposed_port))
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
        let mut client = self
            .state
            .get_scheduler_client()
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        let resp = client.deploy_app(req).await?.into_inner();
        Ok(resp)
    }
}

pub struct BuildTask {
    pub deployment_id: Uuid,
    pub app_id: Uuid,
    pub app_name: String,
    pub user_id: String,
    pub build_id: String,
    pub vcpus: u32,
    pub memory_mib: u32,
    pub disk_mib: u32,
    pub port: u32,
    pub env: HashMap<String, String>,
}

pub async fn start_build_polling(state: AppState, task: BuildTask) {
    let builder = Arc::new(RealBuilderClient {
        addr: state.builder_addr.clone(),
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
    info!("Checking for pending builds to resume...");
    // We only resume builds for apps that are already registered.
    let apps = match state.app_repo.list_apps_by_user("all").await {
        Ok(apps) => apps,
        Err(e) => {
            error!("Failed to list apps for resuming builds: {}", e);
            return;
        },
    };

    for app in apps {
        let deployments = match state.app_repo.list_deployments_by_app(app.id).await {
            Ok(deps) => deps,
            Err(e) => {
                error!("Failed to list deployments for app {}: {}", app.id, e);
                continue;
            },
        };

        for dep in deployments {
            if dep.status == "BUILDING"
                && let Some(build_id) = dep.build_id
            {
                info!(
                    app = %app.name,
                    deployment_id = %dep.id,
                    build_id = %build_id,
                    "Resuming build polling"
                );

                let env: HashMap<String, String> =
                    serde_json::from_value(dep.env_vars.clone()).unwrap_or_default();

                let task = BuildTask {
                    deployment_id: dep.id,
                    app_id: app.id,
                    app_name: app.name.clone(),
                    user_id: dep.user_id.to_string(),
                    build_id,
                    vcpus: dep.vcpus as u32,
                    memory_mib: dep.memory_mib as u32,
                    disk_mib: dep.disk_mib as u32,
                    port: dep.port as u32,
                    env,
                };

                start_build_polling(state.clone(), task).await;
            }
        }
    }
}

async fn poll_and_deploy(
    state: AppState,
    task: BuildTask,
    builder: Arc<dyn BuilderClient>,
    scheduler: Arc<dyn SchedulerClient>,
) -> anyhow::Result<()> {
    // Acquire permit to limit concurrent builds
    let _permit = state.build_semaphore.acquire().await.map_err(|e| {
        error!("Failed to acquire build permit: {}", e);
        anyhow::anyhow!("Build system overloaded")
    })?;

    info!(
        app = %task.app_name,
        build_id = %task.build_id,
        "Starting background polling for build"
    );

    let mut attempts = 0;
    let (final_image, detected_port) = loop {
        if attempts > 60 {
            let _ = state
                .app_repo
                .update_deployment_status(
                    task.deployment_id,
                    "FAILED",
                    None,
                    None,
                    Some(task.build_id.clone()),
                    None,
                )
                .await;
            state.deployment_events.send(task.app_id).ok();
            return Err(anyhow::anyhow!("Build timed out"));
        }

        let (status, image_tag, exposed_port) =
            builder.get_build_status(task.build_id.clone()).await?;

        match status {
            BuildStatus::Success => {
                info!(
                app = %task.app_name,
                image = %image_tag,
                port = %exposed_port,
                "Build successful, proceeding to deployment"
                );
                break (image_tag, exposed_port);
            },
            BuildStatus::Failed => {
                let _ = state
                    .app_repo
                    .update_deployment_status(
                        task.deployment_id,
                        "FAILED",
                        None,
                        None,
                        Some(task.build_id.clone()),
                        None,
                    )
                    .await;
                state.deployment_events.send(task.app_id).ok();
                return Err(anyhow::anyhow!("Build failed"));
            },
            _ => {
                attempts += 1;
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            },
        }
    };

    // Use detected port if available (Dockerfile EXPOSE), otherwise use original port (Railpack/Default)
    let final_port = if detected_port > 0 {
        info!(app = %task.app_name, port = %detected_port, "Using detected port from image");
        detected_port
    } else {
        task.port
    };

    let deploy_req = DeployRequest {
        app_id: task.app_id.to_string(),
        app_name: task.app_name.clone(),
        image: final_image.clone(),
        config: Some(AppConfig {
            vcpus: task.vcpus,
            memory_mib: task.memory_mib,
            disk_mib: task.disk_mib,
            port: final_port,
            env: task.env,
            ip_address: String::new(),
            gateway: String::new(),
            mac_address: String::new(),
            volumes: vec![], // TODO: Support volumes in background task
        }),
        user_id: task.user_id.clone(),
    };

    let response = scheduler.deploy_app(deploy_req).await?;

    // Update App record with the detected port if it changed
    if detected_port > 0 && detected_port != task.port {
        let _ = state
            .app_repo
            .update_app_port(task.app_id, detected_port as i32)
            .await;
    }

    // Update Deployment with Scheduler info
    let _ = state
        .app_repo
        .update_deployment_status(
            task.deployment_id,
            "RUNNING",
            Some(response.job_id),
            Some(final_image),
            Some(task.build_id),
            None,
        )
        .await;

    state.deployment_events.send(task.app_id).ok();

    // The deployment record also has a port field (used by router join)
    if detected_port > 0 {
        let _ = state
            .app_repo
            .update_deployment_port(task.deployment_id, detected_port as i32)
            .await;
    }

    // Auto-promote if no active deployment is set
    if let Ok(Some(app)) = state.app_repo.get_app(task.app_id).await
        && app.active_deployment_id.is_none()
    {
        info!(
            app = %task.app_name,
            deployment_id = %task.deployment_id,
            "No active deployment set, auto-promoting this one"
        );
        let _ = state
            .app_repo
            .set_active_deployment(task.app_id, task.deployment_id)
            .await;
    }

    info!(
        app = %task.app_name,
        "Application deployed successfully in background"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::app_repository::MockAppRepository;
    use mikrom_proto::scheduler::DeployResponse;

    struct MockBuilder {
        status: BuildStatus,
        tag: String,
        port: u32,
    }

    #[async_trait]
    impl BuilderClient for MockBuilder {
        async fn get_build_status(&self, _: String) -> anyhow::Result<(BuildStatus, String, u32)> {
            Ok((self.status, self.tag.clone(), self.port))
        }
    }

    struct MockSchedulerClientImpl {
        success: bool,
    }

    #[async_trait]
    impl SchedulerClient for MockSchedulerClientImpl {
        async fn deploy_app(&self, _: DeployRequest) -> anyhow::Result<DeployResponse> {
            if self.success {
                Ok(DeployResponse {
                    job_id: "job-1".to_string(),
                    status: 1, // Running/Scheduled
                    host_id: "host-1".to_string(),
                    vm_id: "vm-1".to_string(),
                    message: "ok".to_string(),
                })
            } else {
                Err(anyhow::anyhow!("failed"))
            }
        }
    }

    #[tokio::test]
    async fn test_poll_and_deploy_success() {
        let mut mock_repo = MockAppRepository::new();
        let app_id = Uuid::new_v4();
        let dep_id = Uuid::new_v4();

        // 1. Success expectations
        mock_repo
            .expect_update_deployment_status()
            .times(1)
            .returning(|_, _, _, _, _, _| Ok(()));

        mock_repo.expect_get_app().returning(move |_| {
            Ok(Some(crate::models::app::App {
                id: app_id,
                name: "test".into(),
                git_url: "".into(),
                port: 8080,
                hostname: None,
                user_id: Uuid::new_v4(),
                github_webhook_secret: None,
                active_deployment_id: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            }))
        });

        mock_repo
            .expect_set_active_deployment()
            .returning(|_, _| Ok(()));

        let state = AppState {
            user_repo: Arc::new(crate::repositories::user_repository::MockUserRepository::new()),
            app_repo: Arc::new(mock_repo),
            scheduler_client: None,
            scheduler_config: crate::scheduler::SchedulerConfig {
                addr: "".into(),
                use_tls: false,
                certs_dir: None,
            },
            builder_addr: "".into(),
            jwt_secret: "".into(),
            master_key: "".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            build_semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
        };

        let task = BuildTask {
            deployment_id: dep_id,
            app_id,
            app_name: "test".into(),
            user_id: "user-1".into(),
            build_id: "build-1".into(),
            vcpus: 1,
            memory_mib: 256,
            disk_mib: 1024,
            port: 8080,
            env: HashMap::new(),
        };

        let builder = Arc::new(MockBuilder {
            status: BuildStatus::Success,
            tag: "img:v1".into(),
            port: 0,
        });

        let scheduler = Arc::new(MockSchedulerClientImpl { success: true });

        let result = poll_and_deploy(state, task, builder, scheduler).await;
        assert!(result.is_ok());
    }
}
