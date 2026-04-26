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

    async fn delete_app(
        &self,
        req: mikrom_proto::scheduler::DeleteAppRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::DeleteAppResponse> {
        let mut client = self
            .state
            .get_scheduler_client()
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        let resp = client.delete_app(req).await?.into_inner();
        Ok(resp)
    }

    async fn pause_app(
        &self,
        req: mikrom_proto::scheduler::PauseRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::PauseResponse> {
        let mut client = self
            .state
            .get_scheduler_client()
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        let resp = client.pause_app(req).await?.into_inner();
        Ok(resp)
    }

    async fn resume_app(
        &self,
        req: mikrom_proto::scheduler::ResumeRequest,
    ) -> anyhow::Result<mikrom_proto::scheduler::ResumeResponse> {
        let mut client = self
            .state
            .get_scheduler_client()
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        let resp = client.resume_app(req).await?.into_inner();
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

    // Enforce 5 deployments limit
    if let Ok(deployments) = state.app_repo.list_deployments_by_app(task.app_id).await
        && deployments.len() > 5
    {
        let mut deps = deployments;
        deps.sort_by_key(|d| d.created_at);

        let active_id = state
            .app_repo
            .get_app(task.app_id)
            .await
            .ok()
            .flatten()
            .and_then(|a| a.active_deployment_id);

        let to_delete_count = deps.len() - 5;
        let mut deleted = 0;

        for dep in deps {
            if deleted >= to_delete_count {
                break;
            }

            // Never delete the currently active deployment or the one we just created
            if Some(dep.id) == active_id || dep.id == task.deployment_id {
                continue;
            }

            if let Some(job_id) = dep.job_id {
                info!(
                    app = %task.app_name,
                    old_job_id = %job_id,
                    "Deleting oldest deployment to enforce limit"
                );

                // 1. Delete from scheduler (kills VM and wipes resources)
                let _ = scheduler
                    .delete_app(mikrom_proto::scheduler::DeleteAppRequest {
                        job_id: job_id.clone(),
                        user_id: task.user_id.clone(),
                    })
                    .await;

                // 2. Delete from DB
                let _ = state.app_repo.delete_deployment_by_job_id(&job_id).await;
                deleted += 1;
            }
        }
    }

    state.deployment_events.send(task.app_id).ok();

    // The deployment record also has a port field (used by router join)
    if detected_port > 0 {
        let _ = state
            .app_repo
            .update_deployment_port(task.deployment_id, detected_port as i32)
            .await;
    }

    // 4. Promotion & Cleanup: Promote the new deployment and hibernate ALL others
    info!(
        app = %task.app_name,
        deployment_id = %task.deployment_id,
        "Promoting new deployment to active and hibernating others"
    );

    // Set new one as active
    let _ = state
        .app_repo
        .set_active_deployment(task.app_id, task.deployment_id)
        .await;

    // Hibernate ALL other non-terminal deployments
    if let Ok(all_deployments) = state.app_repo.list_deployments_by_app(task.app_id).await {
        for dep in all_deployments {
            // Skip the current deployment and already terminal ones
            if dep.id == task.deployment_id
                || ["STOPPED", "FAILED", "CANCELLED"].contains(&dep.status.as_str())
            {
                continue;
            }

            if let Some(old_job_id) = dep.job_id {
                info!(
                    app = %task.app_name,
                    old_deployment_id = %dep.id,
                    "Hibernating previous deployment for exclusivity"
                );

                let mut success = false;
                match scheduler
                    .pause_app(mikrom_proto::scheduler::PauseRequest {
                        job_id: old_job_id.clone(),
                        user_id: task.user_id.clone(),
                    })
                    .await
                {
                    Ok(resp) => {
                        success = resp.success;
                    },
                    Err(e) => {
                        if e.to_string().contains("not found") {
                            success = true;
                        } else {
                            error!(app = %task.app_name, job_id = %old_job_id, "Failed to hibernate old instance: {}", e);
                        }
                    },
                }

                if success {
                    let _ = state
                        .app_repo
                        .update_deployment_status(
                            dep.id,
                            "STOPPED",
                            Some(old_job_id),
                            dep.image_tag,
                            dep.build_id,
                            None,
                        )
                        .await;
                }
            }
        }
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
    use crate::models::app::Deployment;
    use crate::repositories::app_repository::MockAppRepository;
    use mikrom_proto::scheduler::DeployResponse;
    use mockall::predicate::*;

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

        async fn delete_app(
            &self,
            _: mikrom_proto::scheduler::DeleteAppRequest,
        ) -> anyhow::Result<mikrom_proto::scheduler::DeleteAppResponse> {
            Ok(mikrom_proto::scheduler::DeleteAppResponse {
                success: true,
                message: "ok".to_string(),
            })
        }

        async fn pause_app(
            &self,
            _: mikrom_proto::scheduler::PauseRequest,
        ) -> anyhow::Result<mikrom_proto::scheduler::PauseResponse> {
            Ok(mikrom_proto::scheduler::PauseResponse {
                success: true,
                message: "ok".to_string(),
            })
        }

        async fn resume_app(
            &self,
            _: mikrom_proto::scheduler::ResumeRequest,
        ) -> anyhow::Result<mikrom_proto::scheduler::ResumeResponse> {
            Ok(mikrom_proto::scheduler::ResumeResponse {
                success: true,
                message: "ok".to_string(),
            })
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
            .returning(|_, _, _, _, _, _| Ok(()));

        mock_repo
            .expect_list_deployments_by_app()
            .returning(move |_| Ok(vec![]));

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

    #[tokio::test]
    async fn test_poll_and_deploy_enforces_limit() {
        let mut mock_repo = MockAppRepository::new();
        let app_id = Uuid::new_v4();
        let dep_id_new = Uuid::new_v4();
        let dep_id_oldest = Uuid::new_v4();
        let dep_id_active = Uuid::new_v4();

        let now = chrono::Utc::now();

        // Prepare 5 existing deployments
        let mut existing_deps = vec![];
        // Oldest one
        existing_deps.push(Deployment {
            id: dep_id_oldest,
            app_id,
            user_id: Uuid::new_v4(),
            status: "RUNNING".to_string(),
            job_id: Some("job-oldest".to_string()),
            created_at: now - chrono::Duration::days(10),
            ..Default::default()
        });
        // Active one
        existing_deps.push(Deployment {
            id: dep_id_active,
            app_id,
            user_id: Uuid::new_v4(),
            status: "RUNNING".to_string(),
            job_id: Some("job-active".to_string()),
            created_at: now - chrono::Duration::days(5),
            ..Default::default()
        });
        // 3 more
        for i in 0..3 {
            existing_deps.push(Deployment {
                id: Uuid::new_v4(),
                app_id,
                user_id: Uuid::new_v4(),
                status: "RUNNING".to_string(),
                job_id: Some(format!("job-{}", i)),
                created_at: now - chrono::Duration::days(i),
                ..Default::default()
            });
        }

        mock_repo
            .expect_update_deployment_status()
            .returning(|_, _, _, _, _, _| Ok(()));

        mock_repo
            .expect_update_deployment_port()
            .returning(|_, _| Ok(()));

        mock_repo
            .expect_set_active_deployment()
            .returning(|_, _| Ok(()));

        mock_repo.expect_get_deployment().returning(move |_| {
            Ok(Some(Deployment {
                id: dep_id_active,
                job_id: Some("job-active".to_string()),
                ..Default::default()
            }))
        });

        // When list_deployments_by_app is called, it returns 6 (5 existing + 1 new)
        let mut all_deps = existing_deps.clone();
        all_deps.push(Deployment {
            id: dep_id_new,
            app_id,
            created_at: now,
            ..Default::default()
        });

        mock_repo
            .expect_list_deployments_by_app()
            .returning(move |_| Ok(all_deps.clone()));

        mock_repo.expect_get_app().returning(move |_| {
            Ok(Some(crate::models::app::App {
                id: app_id,
                active_deployment_id: Some(dep_id_active),
                ..Default::default()
            }))
        });

        // EXPECTATION: Delete the oldest deployment
        mock_repo
            .expect_delete_deployment_by_job_id()
            .with(mockall::predicate::eq("job-oldest"))
            .times(1)
            .returning(|_| Ok(()));

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
            deployment_id: dep_id_new,
            app_id,
            app_name: "test".into(),
            user_id: "user-1".into(),
            build_id: "build-1".into(),
            vcpus: 1,
            memory_mib: 128,
            disk_mib: 512,
            port: 8080,
            env: HashMap::new(),
        };

        let builder = Arc::new(MockBuilder {
            status: BuildStatus::Success,
            tag: "tag".into(),
            port: 8080,
        });

        let scheduler = Arc::new(MockSchedulerClientImpl { success: true });

        let result = poll_and_deploy(state, task, builder, scheduler).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_poll_and_deploy_pauses_previous_active() {
        let mut mock_repo = MockAppRepository::new();
        let app_id = Uuid::new_v4();
        let dep_id_new = Uuid::new_v4();
        let dep_id_prev = Uuid::new_v4();
        let user_id = Uuid::new_v4();

        mock_repo.expect_get_app().returning(move |_| {
            Ok(Some(crate::models::app::App {
                id: app_id,
                active_deployment_id: Some(dep_id_prev),
                ..Default::default()
            }))
        });

        // Mock getting the previous deployment to find its job_id
        mock_repo
            .expect_get_deployment()
            .with(mockall::predicate::eq(dep_id_prev))
            .returning(move |_| {
                Ok(Some(Deployment {
                    id: dep_id_prev,
                    job_id: Some("job-prev".to_string()),
                    ..Default::default()
                }))
            });

        mock_repo
            .expect_set_active_deployment()
            .with(
                mockall::predicate::eq(app_id),
                mockall::predicate::eq(dep_id_new),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // EXPECTATION: Update the previous deployment status to STOPPED
        mock_repo
            .expect_update_deployment_status()
            .with(
                mockall::predicate::eq(dep_id_prev),
                mockall::predicate::eq("STOPPED"),
                always(),
                always(),
                always(),
                always(),
            )
            .times(1)
            .returning(|_, _, _, _, _, _| Ok(()));

        // Also the new one being set to RUNNING
        mock_repo
            .expect_update_deployment_status()
            .with(
                mockall::predicate::eq(dep_id_new),
                mockall::predicate::eq("RUNNING"),
                always(),
                always(),
                always(),
                always(),
            )
            .returning(|_, _, _, _, _, _| Ok(()));

        mock_repo
            .expect_update_deployment_port()
            .returning(|_, _| Ok(()));

        // Mock listing deployments to include the previous one for the "pause all others" logic
        let dep_prev = Deployment {
            id: dep_id_prev,
            status: "RUNNING".to_string(),
            job_id: Some("job-prev".to_string()),
            ..Default::default()
        };
        mock_repo
            .expect_list_deployments_by_app()
            .returning(move |_| Ok(vec![dep_prev.clone()]));

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
            deployment_id: dep_id_new,
            app_id,
            app_name: "test".into(),
            user_id: user_id.to_string(),
            build_id: "build-1".into(),
            vcpus: 1,
            memory_mib: 128,
            disk_mib: 512,
            port: 8080,
            env: HashMap::new(),
        };

        let builder = Arc::new(MockBuilder {
            status: BuildStatus::Success,
            tag: "tag".into(),
            port: 8080,
        });

        let scheduler = Arc::new(MockSchedulerClientImpl { success: true });

        let result = poll_and_deploy(state, task, builder, scheduler).await;
        assert!(result.is_ok());
    }
}
