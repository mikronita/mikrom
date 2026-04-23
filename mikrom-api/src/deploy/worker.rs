use crate::AppState;
use mikrom_proto::builder::{BuildStatus, BuilderServiceClient, GetBuildStatusRequest};
use mikrom_proto::scheduler::{AppConfig, DeployRequest, SchedulerServiceClient};
use std::collections::HashMap;
use tracing::{error, info};
use uuid::Uuid;

pub struct BuildTask {
    pub deployment_id: Uuid,
    pub app_id: Uuid,
    pub app_name: String,
    pub user_id: String,
    pub build_id: String,
    pub vcpus: u32,
    pub memory_mib: u32,
    pub disk_mib: u32,
    pub env: HashMap<String, String>,
}

pub async fn start_build_polling(state: AppState, task: BuildTask) {
    tokio::spawn(async move {
        if let Err(e) = poll_and_deploy(state, task).await {
            error!("Background build/deploy task failed: {}", e);
        }
    });
}

pub async fn resume_pending_builds(state: AppState) {
    info!("Checking for pending builds to resume...");
    // We only resume builds for apps that are already registered.
    // list_apps_by_user("all") is a bit of a hack to get all apps.
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
                    env,
                };
                start_build_polling(state.clone(), task).await;
            }
        }
    }
}

async fn poll_and_deploy(state: AppState, task: BuildTask) -> anyhow::Result<()> {
    info!(
        app = %task.app_name,
        build_id = %task.build_id,
        "Starting background polling for build"
    );

    let builder_channel = crate::builder::connect(&state.builder_addr).await?;
    let mut builder_client = BuilderServiceClient::new(builder_channel);

    let mut attempts = 0;
    let final_image = loop {
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
            return Err(anyhow::anyhow!("Build timed out"));
        }

        let status_resp = builder_client
            .get_build_status(GetBuildStatusRequest {
                build_id: task.build_id.clone(),
            })
            .await?
            .into_inner();

        match BuildStatus::try_from(status_resp.status).unwrap_or(BuildStatus::Unspecified) {
            BuildStatus::Success => {
                info!(
                    app = %task.app_name,
                    image = %status_resp.image_tag,
                    "Build successful, proceeding to deployment"
                );
                break status_resp.image_tag;
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
                return Err(anyhow::anyhow!("Build failed: {}", status_resp.message));
            },
            _ => {
                attempts += 1;
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            },
        }
    };

    // Build success -> Schedule VM
    let scheduler_channel = crate::scheduler::connect(&state.scheduler_config)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to scheduler: {}", e))?;
    let mut scheduler_client = SchedulerServiceClient::new(scheduler_channel);

    let deploy_req = DeployRequest {
        app_id: task.app_id.to_string(),
        app_name: task.app_name.clone(),
        image: final_image.clone(),
        config: Some(AppConfig {
            vcpus: task.vcpus,
            memory_mib: task.memory_mib,
            disk_mib: task.disk_mib,
            env: task.env,
            ip_address: String::new(),
            gateway: String::new(),
            mac_address: String::new(),
            volumes: vec![], // TODO: Support volumes in background task
        }),
        user_id: task.user_id.clone(),
    };

    let response = scheduler_client
        .deploy_app(deploy_req)
        .await
        .map_err(|e| anyhow::anyhow!("Scheduler deploy failed: {}", e))?
        .into_inner();

    // Update Deployment with Scheduler info
    let _ = state
        .app_repo
        .update_deployment_status(
            task.deployment_id,
            "RUNNING",
            Some(response.job_id),
            Some(final_image),
            Some(task.build_id),
            None, // IP will be synced by the other background task
        )
        .await;

    info!(app = %task.app_name, "Application deployed successfully in background");
    Ok(())
}
