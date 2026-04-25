use crate::AppState;
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::models::app::Deployment;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::sse::{Event, Sse},
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateAppRequest {
    pub name: String,
    pub git_url: String,
    pub port: Option<i32>,
    pub hostname: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AppResponse {
    pub id: Uuid,
    pub name: String,
    pub git_url: String,
    pub port: i32,
    pub hostname: Option<String>,
    pub github_webhook_secret: Option<String>,
    pub active_deployment_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[utoipa::path(
    post,
    path = "/apps",
    request_body = CreateAppRequest,
    responses(
        (status = 201, description = "App created successfully", body = AppResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "apps",
    security(
        ("jwt" = [])
    )
)]
pub async fn create_app_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<CreateAppRequest>,
) -> ApiResult<Json<AppResponse>> {
    let port = payload.port.unwrap_or(8080);

    // Generate hostname based on app name if not provided
    let sanitized_name = payload
        .name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    let hostname = Some(format!("{}.apps.mikrom.es", sanitized_name));

    let webhook_secret = Some(uuid::Uuid::new_v4().to_string().replace("-", ""));

    let app = state
        .app_repo
        .create_app(
            &payload.name,
            &payload.git_url,
            port,
            hostname,
            &auth.user_id,
            webhook_secret,
        )
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(Json(AppResponse {
        id: app.id,
        name: app.name,
        git_url: app.git_url,
        port: app.port,
        hostname: app.hostname,
        github_webhook_secret: app.github_webhook_secret,
        active_deployment_id: app.active_deployment_id,
        created_at: app.created_at,
    }))
}

#[utoipa::path(
    get,
    path = "/apps",
    responses(
        (status = 200, description = "List of user apps", body = [AppResponse]),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "apps",
    security(
        ("jwt" = [])
    )
)]
pub async fn list_apps_handler(
    auth: AuthUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<AppResponse>>> {
    let apps = state
        .app_repo
        .list_apps_by_user(&auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let resp: Vec<AppResponse> = apps
        .into_iter()
        .map(|app| AppResponse {
            id: app.id,
            name: app.name,
            git_url: app.git_url,
            port: app.port,
            hostname: app.hostname,
            github_webhook_secret: app.github_webhook_secret,
            active_deployment_id: app.active_deployment_id,
            created_at: app.created_at,
        })
        .collect();

    Ok(Json(resp))
}

#[utoipa::path(
    delete,
    path = "/apps/{app_id}",
    params(
        ("app_id" = Uuid, Path, description = "App ID")
    ),
    responses(
        (status = 204, description = "App deleted successfully"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "App not found", body = crate::error::ErrorResponse)
    ),
    tag = "apps",
    security(
        ("jwt" = [])
    )
)]
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

#[derive(Debug, Deserialize, ToSchema)]
pub struct ManualDeployRequest {
    pub vcpus: Option<u32>,
    pub memory_mib: Option<u64>,
    pub disk_mib: Option<u64>,
    pub env: Option<std::collections::HashMap<String, String>>,
}

#[utoipa::path(
    post,
    path = "/apps/{app_id}/deploy",
    params(
        ("app_id" = Uuid, Path, description = "App ID")
    ),
    request_body = ManualDeployRequest,
    responses(
        (status = 202, description = "Deployment triggered", body = crate::deploy::DeployResponseBody),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "App not found", body = crate::error::ErrorResponse)
    ),
    tag = "apps",
    security(
        ("jwt" = [])
    )
)]
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
    let vcpus = payload.vcpus.unwrap_or(1);
    let memory_mib = payload.memory_mib.unwrap_or(256);
    let disk_mib = payload.disk_mib.unwrap_or(1024);
    let env_vars = payload.env.clone().unwrap_or_default();

    let deployment = state
        .app_repo
        .create_deployment(crate::repositories::app_repository::NewDeployment {
            app_id: app.id,
            user_id: auth.user_id.clone(),
            vcpus: vcpus as i32,
            memory_mib: memory_mib as i64,
            disk_mib: disk_mib as i64,
            port: app.port,
            env_vars: env_vars.clone(),
        })
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

    // Notify update
    state.deployment_events.send(app.id).ok();

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

    // Start background polling
    let task = crate::deploy::worker::BuildTask {
        deployment_id: deployment.id,
        app_id: app.id,
        app_name: app.name.clone(),
        user_id: auth.user_id.clone(),
        build_id: build_id.clone(),
        vcpus,
        memory_mib: memory_mib as u32,
        disk_mib: disk_mib as u32,
        port: app.port as u32,
        env: env_vars,
    };

    crate::deploy::worker::start_build_polling(state.clone(), task).await;

    Ok(Json(crate::deploy::DeployResponseBody {
        job_id: None,
        deployment_id: Some(deployment.id),
        status: "BUILDING".to_string(),
        host_id: None,
        vm_id: None,
        image_tag: None,
        message: "Build initiated in background".to_string(),
    }))
}

/// Helper function to trigger a build and deployment for an app.
/// Reuses logic from deploy_app_version_handler.
pub async fn trigger_app_build(
    state: crate::AppState,
    app: crate::models::app::App,
) -> ApiResult<Uuid> {
    // 1. Create deployment record in DB (using defaults)
    let vcpus = 1;
    let memory_mib = 256;
    let disk_mib = 1024;
    let env_vars = std::collections::HashMap::new();

    let deployment = state
        .app_repo
        .create_deployment(crate::repositories::app_repository::NewDeployment {
            app_id: app.id,
            user_id: app.user_id.to_string(),
            vcpus,
            memory_mib: memory_mib as i64,
            disk_mib: disk_mib as i64,
            port: app.port,
            env_vars: env_vars.clone(),
        })
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    // 2. Trigger build and deploy
    let git_url = app.git_url.clone();
    let app_name = app.name.clone();

    state
        .app_repo
        .update_deployment_status(deployment.id, "BUILDING", None, None, None, None)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    // Notify update
    state.deployment_events.send(app.id).ok();

    // Connect to builder
    let builder_channel = crate::builder::connect(&state.builder_addr)
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to connect to builder: {}", e)))?;

    let mut builder_client = mikrom_proto::builder::BuilderServiceClient::new(builder_channel);

    let build_req = mikrom_proto::builder::BuildRequest {
        app_id: app.id.to_string(),
        git_url: git_url.clone(),
        image_name: app_name.to_lowercase().replace(" ", "-"),
        tag: deployment.id.to_string(),
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

    // Start background polling
    let task = crate::deploy::worker::BuildTask {
        deployment_id: deployment.id,
        app_id: app.id,
        app_name: app.name.clone(),
        user_id: app.user_id.to_string(),
        build_id: build_id.clone(),
        vcpus: vcpus as u32,
        memory_mib: memory_mib as u32,
        disk_mib: disk_mib as u32,
        port: app.port as u32,
        env: env_vars,
    };

    crate::deploy::worker::start_build_polling(state, task).await;

    Ok(deployment.id)
}

#[utoipa::path(
    get,
    path = "/apps/{app_id}/deployments",
    params(
        ("app_id" = Uuid, Path, description = "App ID")
    ),
    responses(
        (status = 200, description = "List of app deployments", body = [Deployment]),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "App not found", body = crate::error::ErrorResponse)
    ),
    tag = "apps",
    security(
        ("jwt" = [])
    )
)]
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

#[utoipa::path(
    get,
    path = "/apps/{app_id}/deployments/stream",
    params(
        ("app_id" = Uuid, Path, description = "App ID"),
        ("token" = String, Query, description = "JWT Token")
    ),
    responses(
        (status = 200, description = "SSE stream for deployments"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "App not found")
    ),
    tag = "apps",
    security(
        ("jwt" = [])
    )
)]
pub async fn deployments_stream_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(app_id): Path<Uuid>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
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

    let app_repo = state.app_repo.clone();
    let receiver = state.deployment_events.subscribe();

    let stream = async_stream::stream! {
        // Initial event
        if let Some(json) = app_repo.list_deployments_by_app(app_id).await.ok().and_then(|d| serde_json::to_string(&d).ok()) {
            yield Ok(Event::default().data(json));
        }

        let mut broadcast_stream = BroadcastStream::new(receiver);
        while let Some(result) = broadcast_stream.next().await {
            match result {
                Ok(event_app_id) if event_app_id == app_id => {
                    if let Some(json) = app_repo.list_deployments_by_app(app_id).await.ok().and_then(|d| serde_json::to_string(&d).ok()) {
                        yield Ok(Event::default().data(json));
                    }
                }
                _ => {} // Ignore errors or other app_ids
            }
        }
    };

    Ok(Sse::new(stream))
}

#[utoipa::path(
    post,
    path = "/apps/{app_id}/deployments/{deployment_id}/activate",
    params(
        ("app_id" = Uuid, Path, description = "App ID"),
        ("deployment_id" = Uuid, Path, description = "Deployment ID")
    ),
    responses(
        (status = 200, description = "Deployment activated"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "App or Deployment not found", body = crate::error::ErrorResponse),
        (status = 400, description = "Deployment is not in RUNNING status", body = crate::error::ErrorResponse)
    ),
    tag = "apps",
    security(
        ("jwt" = [])
    )
)]
pub async fn activate_deployment_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path((app_id, deployment_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    // 1. Verify app ownership
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

    // 2. Verify deployment exists and belongs to app
    let deployment = state
        .app_repo
        .get_deployment(deployment_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or(ApiError::NotFound("Deployment not found".to_string()))?;

    if deployment.app_id != app_id {
        return Err(ApiError::BadRequest(
            "Deployment does not belong to this app".to_string(),
        ));
    }

    if deployment.status != "RUNNING" {
        return Err(ApiError::BadRequest(
            "Only RUNNING deployments can be activated".to_string(),
        ));
    }

    // 3. Set as active
    state
        .app_repo
        .set_active_deployment(app_id, deployment_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    // 4. Cleanup old instances (Auto-stop/delete old firecracker instances)
    let old_deployments = state
        .app_repo
        .list_deployments_by_app(app_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let state_clone = state.clone();
    let user_id_str = auth.user_id.clone();

    tokio::spawn(async move {
        for old_dep in old_deployments {
            // Skip the newly activated one or non-active ones
            if old_dep.id == deployment_id
                || (old_dep.status != "RUNNING" && old_dep.status != "PENDING")
            {
                continue;
            }

            if let Some(job_id) = old_dep.job_id {
                tracing::info!(app_id = %app_id, old_job_id = %job_id, "Cleaning up old instance after promotion");

                // Best effort delete from scheduler
                if let Ok(mut client) = state_clone.get_scheduler_client().await {
                    let _ = client
                        .delete_app(mikrom_proto::scheduler::DeleteAppRequest {
                            job_id: job_id.clone(),
                            user_id: user_id_str.clone(),
                        })
                        .await;
                }

                // Mark as STOPPED in database
                let _ = state_clone
                    .app_repo
                    .update_deployment_status(
                        old_dep.id,
                        "STOPPED",
                        Some(job_id),
                        old_dep.image_tag,
                        old_dep.build_id,
                        None,
                    )
                    .await;
            }
        }
    });

    // Notify update
    state.deployment_events.send(app_id).ok();

    Ok(StatusCode::OK)
}
