use crate::AppState;
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::models::app::Deployment;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct CreateAppRequest {
    pub name: String,
    pub git_url: String,
    pub port: Option<i32>,
    pub hostname: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AppResponse {
    pub id: Uuid,
    pub name: String,
    pub git_url: String,
    pub port: i32,
    pub hostname: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn create_app_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<CreateAppRequest>,
) -> ApiResult<Json<AppResponse>> {
    let port = payload.port.unwrap_or(8080);

    // Generate hostname based on app name if not provided
    let hostname = Some(format!(
        "{}.apps.mikrom.es",
        payload.name.to_lowercase().replace(" ", "-")
    ));

    let app = state
        .app_repo
        .create_app(
            &payload.name,
            &payload.git_url,
            port,
            hostname,
            &auth.user_id,
        )
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(Json(AppResponse {
        id: app.id,
        name: app.name,
        git_url: app.git_url,
        port: app.port,
        hostname: app.hostname,
        created_at: app.created_at,
    }))
}

pub async fn list_apps_handler(
    auth: AuthUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<AppResponse>>> {
    let apps = state
        .app_repo
        .list_apps_by_user(&auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let resp = apps
        .into_iter()
        .map(|app| AppResponse {
            id: app.id,
            name: app.name,
            git_url: app.git_url,
            port: app.port,
            hostname: app.hostname,
            created_at: app.created_at,
        })
        .collect();

    Ok(Json(resp))
}

pub async fn delete_app_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(app_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    // 1. Verify app exists and belongs to user
    let app = state
        .app_repo
        .get_app(app_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or(ApiError::NotFound("App not found".to_string()))?;

    if app.user_id
        != Uuid::parse_str(&auth.user_id)
            .map_err(|_| ApiError::Internal("Invalid user id".to_string()))?
    {
        return Err(ApiError::Forbidden);
    }

    // 2. Delete the app (cascading will handle deployments)
    state
        .app_repo
        .delete_app(app_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub struct ManualDeployRequest {
    pub vcpus: Option<u32>,
    pub memory_mib: Option<u64>,
    pub disk_mib: Option<u64>,
    pub env: Option<std::collections::HashMap<String, String>>,
}

pub async fn deploy_app_version_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(app_id): Path<Uuid>,
    Json(payload): Json<ManualDeployRequest>,
) -> ApiResult<Json<crate::deploy::DeployResponseBody>> {
    // 1. Verify app exists and belongs to user
    let app = state
        .app_repo
        .get_app(app_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or(ApiError::NotFound("App not found".to_string()))?;

    if app.user_id
        != Uuid::parse_str(&auth.user_id)
            .map_err(|_| ApiError::Internal("Invalid user id".to_string()))?
    {
        return Err(ApiError::Forbidden);
    }

    // 2. Create deployment record in DB
    let deployment = state
        .app_repo
        .create_deployment(app.id, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    // 3. Trigger build and deploy
    let git_url = app.git_url.clone();
    let app_name = app.name.clone();

    // We update the deployment status to BUILDING
    state
        .app_repo
        .update_deployment_status(deployment.id, "BUILDING", None, None, None, None)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    // Connect to builder
    let builder_channel = crate::builder::connect(&state.builder_addr)
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to connect to builder: {}", e)))?;

    let mut builder_client = mikrom_proto::builder::BuilderServiceClient::new(builder_channel);

    let build_req = mikrom_proto::builder::BuildRequest {
        app_id: app.id.to_string(),
        git_url: git_url.clone(),
        image_name: app_name.to_lowercase().replace(" ", "-"),
        tag: deployment.id.to_string(), // Use deployment ID as tag for uniqueness
    };

    let build_resp = builder_client
        .build_app(build_req)
        .await
        .map_err(|e| ApiError::Internal(format!("Build initiation failed: {}", e)))?
        .into_inner();

    let build_id = build_resp.build_id;
    state
        .app_repo
        .update_deployment_status(
            deployment.id,
            "BUILDING",
            None,
            None,
            Some(build_id.clone()),
            None,
        )
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    // Polling Build Status
    let final_image;
    let mut attempts = 0;
    loop {
        if attempts > 60 {
            state
                .app_repo
                .update_deployment_status(deployment.id, "FAILED", None, None, None, None)
                .await
                .ok();
            return Err(ApiError::Internal("Build timed out".to_string()));
        }

        let status_resp = builder_client
            .get_build_status(mikrom_proto::builder::GetBuildStatusRequest {
                build_id: build_id.clone(),
            })
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to get build status: {}", e)))?
            .into_inner();

        match mikrom_proto::builder::BuildStatus::try_from(status_resp.status)
            .unwrap_or(mikrom_proto::builder::BuildStatus::Unspecified)
        {
            mikrom_proto::builder::BuildStatus::Success => {
                final_image = status_resp.image_tag;
                break;
            }
            mikrom_proto::builder::BuildStatus::Failed => {
                state
                    .app_repo
                    .update_deployment_status(deployment.id, "FAILED", None, None, None, None)
                    .await
                    .ok();
                return Err(ApiError::Internal(format!(
                    "Build failed: {}",
                    status_resp.message
                )));
            }
            _ => {
                attempts += 1;
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }

    // 4. Schedule VM
    let vcpus = payload.vcpus.unwrap_or(1);
    let memory_mib = payload.memory_mib.unwrap_or(256);
    let disk_mib = payload.disk_mib.unwrap_or(1024);

    let scheduler_channel = crate::scheduler::connect(&state.scheduler_config)
        .await
        .map_err(ApiError::Scheduler)?;

    let mut scheduler_client =
        mikrom_proto::scheduler::SchedulerServiceClient::new(scheduler_channel);

    let deploy_req = mikrom_proto::scheduler::DeployRequest {
        app_id: app.id.to_string(),
        app_name: app.name.clone(),
        image: final_image.clone(),
        config: Some(mikrom_proto::scheduler::AppConfig {
            vcpus,
            memory_mib: memory_mib as u32,
            disk_mib: disk_mib as u32,
            env: payload.env.clone().unwrap_or_default(),
            ip_address: String::new(),
            gateway: String::new(),
            mac_address: String::new(),
            volumes: vec![],
        }),
        user_id: auth.user_id.clone(),
    };

    let response = scheduler_client
        .deploy_app(deploy_req)
        .await
        .map_err(|e| ApiError::Internal(e.message().to_string()))?;

    let inner = response.into_inner();

    // 5. Update Deployment with Scheduler info
    let job_status = scheduler_client
        .get_app_status(mikrom_proto::scheduler::AppStatusRequest {
            job_id: inner.job_id.clone(),
            user_id: auth.user_id.clone(),
        })
        .await
        .ok()
        .map(|r| r.into_inner());

    let ip_address = job_status
        .as_ref()
        .map(|s| s.ip_address.clone())
        .filter(|s| !s.is_empty());

    state
        .app_repo
        .update_deployment_status(
            deployment.id,
            "RUNNING",
            Some(inner.job_id.clone()),
            Some(final_image.clone()),
            None,
            ip_address.clone(),
        )
        .await
        .ok();

    Ok(Json(crate::deploy::DeployResponseBody {
        job_id: inner.job_id,
        status: "RUNNING".to_string(),
        host_id: Some(inner.host_id).filter(|s| !s.is_empty()),
        vm_id: Some(inner.vm_id).filter(|s| !s.is_empty()),
        image_tag: Some(final_image),
        message: "Application deployed successfully".to_string(),
    }))
}

pub async fn list_deployments_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(app_id): Path<Uuid>,
) -> ApiResult<Json<Vec<Deployment>>> {
    // Verify ownership
    let app = state
        .app_repo
        .get_app(app_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or(ApiError::NotFound("App not found".to_string()))?;

    if app.user_id
        != Uuid::parse_str(&auth.user_id)
            .map_err(|_| ApiError::Internal("Invalid user id".to_string()))?
    {
        return Err(ApiError::Forbidden);
    }

    let deployments = state
        .app_repo
        .list_deployments_by_app(app_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(Json(deployments))
}
