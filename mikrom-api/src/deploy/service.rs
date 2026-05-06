use crate::AppState;
use crate::deploy::worker::{BuildTask, start_build_polling};
use crate::error::{ApiError, ApiResult};
use crate::models::app::{App, Deployment};
use crate::repositories::app_repository::UpdateDeploymentParams;
use mikrom_proto::scheduler::{AppConfig, DeployRequest, DeployResponse};

pub struct DeploymentService;

pub struct DeployParams {
    pub image_tag: String,
    pub vcpus: u32,
    pub memory_mib: u32,
    pub disk_mib: u32,
    pub env: std::collections::HashMap<String, String>,
}

impl DeploymentService {
    pub async fn trigger_build(
        state: &AppState,
        app: &App,
        deployment: &Deployment,
        vcpus: u32,
        memory_mib: u64,
        disk_mib: u64,
        env: std::collections::HashMap<String, String>,
    ) -> ApiResult<String> {
        state
            .app_repo
            .update_deployment(
                deployment.id,
                UpdateDeploymentParams {
                    status: Some("BUILDING".to_string()),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        // Notify cluster via NATS for BUILDING phase
        {
            use mikrom_proto::scheduler::AppInfo;
            let info = AppInfo {
                job_id: format!("temp-{}", deployment.id),
                app_id: app.id.to_string(),
                app_name: app.name.clone(),
                image: String::new(),
                status: 1, // Pending/Building
                user_id: app.user_id.to_string(),
                deployment_id: deployment.id.to_string(),
                ..Default::default()
            };
            let _ = state
                .nats
                .publish("mikrom.scheduler.job_updates", info)
                .await;
        }

        state.deployment_events.send(app.id).ok();

        let mut git_auth_token = None;
        if let (Some(installation_id), Some(app_id), Some(private_key)) = (
            app.github_installation_id,
            &state.github_app_id,
            &state.github_private_key,
        ) {
            match crate::github::get_installation_token(app_id, private_key, installation_id).await
            {
                Ok(token) => git_auth_token = Some(token),
                Err(e) => tracing::error!("Failed to get GitHub installation token: {}", e),
            }
        }

        let build_req = mikrom_proto::builder::BuildRequest {
            app_id: app.id.to_string(),
            git_url: app.git_url.clone(),
            image_name: app.name.to_lowercase().replace(' ', "-"),
            tag: deployment.id.to_string(),
            git_auth_token,
        };

        let build_resp: mikrom_proto::builder::BuildResponse = state
            .nats
            .with_timeout(std::time::Duration::from_secs(5))
            .request("mikrom.builder.build", build_req)
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to trigger build via NATS: {}", e)))?;

        let build_id = build_resp.build_id;
        state
            .app_repo
            .update_deployment(
                deployment.id,
                UpdateDeploymentParams {
                    status: Some("BUILDING".to_string()),
                    build_id: Some(build_id.clone()),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        let task = BuildTask {
            deployment_id: deployment.id,
            app_id: app.id,
            app_name: app.name.clone(),
            user_id: app.user_id.to_string(),
            build_id: build_id.clone(),
            vcpus,
            memory_mib,
            disk_mib,
            port: app.port as u32,
            env,
        };

        start_build_polling(state.clone(), task).await;

        Ok(build_id)
    }

    pub async fn deploy_to_scheduler(
        state: &AppState,
        app: &App,
        deployment: &Deployment,
        params: DeployParams,
    ) -> ApiResult<DeployResponse> {
        state
            .app_repo
            .update_deployment(
                deployment.id,
                UpdateDeploymentParams {
                    status: Some("SCHEDULED".to_string()),
                    image_tag: Some(params.image_tag.clone()),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        let nats_req = DeployRequest {
            app_id: app.id.to_string(),
            app_name: app.name.clone(),
            image: params.image_tag.clone(),
            user_id: app.user_id.to_string(),
            config: Some(AppConfig {
                vcpus: params.vcpus,
                memory_mib: params.memory_mib,
                disk_mib: params.disk_mib,
                port: app.port as u32,
                env: params.env,
                ..Default::default()
            }),
            deployment_id: deployment.id.to_string(),
        };

        let nats_result: ApiResult<DeployResponse> = state
            .nats
            .with_timeout(std::time::Duration::from_secs(5))
            .request("mikrom.scheduler.deploy", nats_req)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()));

        let inner = match nats_result {
            Ok(inner) => inner,
            Err(e) => {
                let _ = state
                    .app_repo
                    .update_deployment(
                        deployment.id,
                        UpdateDeploymentParams {
                            status: Some("FAILED".to_string()),
                            image_tag: Some(params.image_tag.clone()),
                            git_branch: Some(e.to_string()),
                            ..Default::default()
                        },
                    )
                    .await;
                state.deployment_events.send(app.id).ok();
                return Err(e);
            },
        };

        let db_status = crate::scheduler::status_name(inner.status);
        state
            .app_repo
            .update_deployment(
                deployment.id,
                UpdateDeploymentParams {
                    status: Some(db_status.to_string()),
                    job_id: Some(inner.job_id.clone()),
                    image_tag: Some(params.image_tag),
                    ip_address: Some(inner.ip_address.clone()),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        state.deployment_events.send(app.id).ok();

        Ok(inner)
    }
}
