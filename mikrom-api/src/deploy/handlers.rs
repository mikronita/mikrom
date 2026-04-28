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
use tracing::info;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateAppRequest {
    pub name: String,
    pub git_url: String,
    pub port: Option<u32>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AppResponse {
    pub id: Uuid,
    pub name: String,
    pub git_url: String,
    pub port: u32,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ManualDeployRequest {
    pub vcpus: Option<u32>,
    pub memory_mib: Option<u32>,
    pub disk_mib: Option<u32>,
    pub env: Option<std::collections::HashMap<String, String>>,
}

#[utoipa::path(
    post,
    path = "/apps",
    request_body = CreateAppRequest,
    responses(
        (status = 201, description = "Application created", body = AppResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 500, description = "Internal error", body = crate::error::ErrorResponse)
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
) -> ApiResult<(StatusCode, Json<AppResponse>)> {
    let port = payload.port.unwrap_or(8080);
    let hostname = format!(
        "{}.apps.mikrom.es",
        payload.name.to_lowercase().replace(' ', "-")
    );

    let app = state
        .app_repo
        .create_app(
            &payload.name,
            &payload.git_url,
            port as i32,
            Some(hostname),
            &auth.user_id,
            None,
        )
        .await
        .map_err(ApiError::from)?;

    Ok((
        StatusCode::CREATED,
        Json(AppResponse {
            id: app.id,
            name: app.name,
            git_url: app.git_url,
            port: app.port as u32,
        }),
    ))
}

#[utoipa::path(
    get,
    path = "/apps",
    responses(
        (status = 200, description = "List applications", body = [AppResponse]),
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

    Ok(Json(
        apps.into_iter()
            .map(|a| AppResponse {
                id: a.id,
                name: a.name,
                git_url: a.git_url,
                port: a.port as u32,
            })
            .collect(),
    ))
}

#[utoipa::path(
    delete,
    path = "/apps/{app_name}",
    params(
        ("app_name" = String, Path, description = "Application name")
    ),
    responses(
        (status = 204, description = "Application deleted"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Application not found", body = crate::error::ErrorResponse)
    ),
    tag = "apps",
    security(
        ("jwt" = [])
    )
)]
pub async fn delete_app_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(app_name): Path<String>,
) -> ApiResult<StatusCode> {
    let app = get_app_by_name_and_auth(&state, &app_name, &auth).await?;

    // Clean up all deployments before deleting the app
    let deployments = state
        .app_repo
        .list_deployments_by_app(app.id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    for dep in deployments {
        if let Some(job_id) = dep.job_id {
            // Tell the scheduler to clean up resources for this job
            if let Err(e) = state
                .scheduler
                .delete_app(job_id, app.user_id.to_string())
                .await
            {
                tracing::error!(
                    "Failed to clean up deployment resources in scheduler: {}",
                    e
                );
            }
        }
    }

    state
        .app_repo
        .delete_app(app.id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/apps/{app_name}/deployments",
    params(
        ("app_name" = String, Path, description = "Application name")
    ),
    responses(
        (status = 200, description = "List deployments", body = [Deployment]),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
pub async fn list_deployments_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(app_name): Path<String>,
) -> ApiResult<Json<Vec<Deployment>>> {
    let app = get_app_by_name_and_auth(&state, &app_name, &auth).await?;
    let deployments = state
        .app_repo
        .list_deployments_by_app(app.id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(Json(deployments))
}

#[utoipa::path(
    get,
    path = "/apps/{app_name}/deployments/stream",
    params(
        ("app_name" = String, Path, description = "Application name")
    ),
    responses(
        (status = 200, description = "SSE stream for deployment updates", content_type = "text/event-stream"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
pub async fn deployments_stream_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(app_name): Path<String>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let app = get_app_by_name_and_auth(&state, &app_name, &auth).await?;
    let app_id = app.id;
    let rx = state.deployment_events.subscribe();
    let state_clone = state.clone();

    // Subscribe to NATS for instant cluster-wide updates
    let nats_sub = state
        .nats_client
        .subscribe("mikrom.scheduler.job_updates")
        .await
        .map_err(|e| ApiError::Internal(format!("NATS sub error: {}", e)))?;

    let stream = async_stream::stream! {
        let mut local_stream = BroadcastStream::new(rx);
        let mut nats_stream = nats_sub;
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));

        // 0. Initial yield: send current state immediately upon connection
        if let Ok(json) = state_clone.app_repo.list_deployments_by_app(app_id).await
            .and_then(|deps| serde_json::to_string(&deps).map_err(|e| anyhow::anyhow!(e))) {
                yield Ok(Event::default().data(json));
        }

        loop {
            tokio::select! {
                // 1. Local events (DB changes)
                res = local_stream.next() => {
                    match res {
                        Some(Ok(id)) if id == app_id => {
                            if let Ok(json) = state_clone.app_repo.list_deployments_by_app(app_id).await
                                .and_then(|deps| serde_json::to_string(&deps).map_err(|e| anyhow::anyhow!(e))) {
                                    yield Ok(Event::default().data(json));
                            }
                        },
                        Some(Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(_))) => {
                            // If we lag, just refresh anyway
                            if let Ok(json) = state_clone.app_repo.list_deployments_by_app(app_id).await
                                .and_then(|deps| serde_json::to_string(&deps).map_err(|e| anyhow::anyhow!(e))) {
                                    yield Ok(Event::default().data(json));
                            }
                        },
                        _ => {}
                    }
                },
                // 2. Cluster events (Scheduler changes)
                Some(msg) = nats_stream.next() => {
                    use prost::Message;
                    use mikrom_proto::scheduler::AppInfo;
                    if let Ok(json) = async {
                        let info = AppInfo::decode(&msg.payload[..])?;
                        if info.app_id != app_id.to_string() { return Err(anyhow::anyhow!("Mismatch")); }
                        let deps = state_clone.app_repo.list_deployments_by_app(app_id).await?;
                        serde_json::to_string(&deps).map_err(|e| anyhow::anyhow!(e))
                    }.await {
                        yield Ok(Event::default().data(json));
                    }
                },
                // 3. Periodic refresh (Brute force fallback to ensure UI stays in sync)
                _ = interval.tick() => {
                    if let Ok(json) = state_clone.app_repo.list_deployments_by_app(app_id).await
                        .and_then(|deps| serde_json::to_string(&deps).map_err(|e| anyhow::anyhow!(e))) {
                            yield Ok(Event::default().data(json));
                    }
                },
                else => break,
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(5))
            .text("keep-alive"),
    ))
}

#[utoipa::path(
    post,
    path = "/apps/{app_name}/deployments/{deployment_id}/activate",
    params(
        ("app_name" = String, Path, description = "Application name"),
        ("deployment_id" = Uuid, Path, description = "Deployment ID")
    ),
    responses(
        (status = 200, description = "Deployment activated"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Deployment not found", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
pub async fn activate_deployment_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path((_app_name, deployment_id)): Path<(String, Uuid)>,
) -> ApiResult<StatusCode> {
    let deployment = state
        .app_repo
        .get_deployment(deployment_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or(ApiError::NotFound("Deployment not found".into()))?;

    // 1. Update active pointer in DB
    state
        .app_repo
        .set_active_deployment(deployment.app_id, deployment_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    // Give the DB a moment to ensure the update is committed before notifying other systems
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // 2. If it has a job_id, ensure it's running
    if let Some(job_id) = deployment.job_id {
        info!(job_id = %job_id, "Activating deployment, ensuring it's running in the cluster...");
        let _ = state.scheduler.resume_app(job_id, auth.user_id).await;
    }

    state.deployment_events.send(deployment.app_id).ok();

    // Notify router
    let _ = state
        .nats_client
        .publish("mikrom.router.config_updated", "refresh".into())
        .await;

    Ok(StatusCode::OK)
}

#[utoipa::path(
    post,
    path = "/apps/{app_name}/deploy",
    params(
        ("app_name" = String, Path, description = "Application name")
    ),
    request_body = ManualDeployRequest,
    responses(
        (status = 200, description = "Deployment triggered", body = crate::deploy::DeployResponseBody),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Application not found", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
pub async fn deploy_app_version_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(app_name): Path<String>,
    Json(payload): Json<ManualDeployRequest>,
) -> ApiResult<Json<crate::deploy::DeployResponseBody>> {
    let app = get_app_by_name_and_auth(&state, &app_name, &auth).await?;

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
            trigger_source: "manual".to_string(),
        })
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    state
        .app_repo
        .update_deployment_status(
            deployment.id,
            "BUILDING",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    // Notify cluster via NATS for BUILDING phase
    {
        use mikrom_proto::scheduler::AppInfo;
        use prost::Message;
        let info = AppInfo {
            job_id: format!("temp-{}", deployment.id),
            app_id: app.id.to_string(),
            app_name: app.name.clone(),
            image: String::new(),
            status: 1, // Pending/Building
            user_id: auth.user_id.clone(),
            deployment_id: deployment.id.to_string(),
            ..Default::default()
        };
        let mut buf = Vec::new();
        if info.encode(&mut buf).is_ok() {
            let _ = state
                .nats_client
                .publish("mikrom.scheduler.job_updates", buf.into())
                .await;
        }
    }

    state.deployment_events.send(app.id).ok();

    use prost::Message;
    let build_req = mikrom_proto::builder::BuildRequest {
        app_id: app.id.to_string(),
        git_url: app.git_url.clone(),
        image_name: app.name.to_lowercase().replace(' ', "-"),
        tag: deployment.id.to_string(),
    };

    let mut buf = Vec::new();
    build_req
        .encode(&mut buf)
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let response = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        state
            .nats_client
            .request("mikrom.builder.build", buf.into()),
    )
    .await
    .map_err(|_| ApiError::Internal("Builder request timed out".into()))?
    .map_err(|e| ApiError::Internal(format!("Failed to trigger build via NATS: {}", e)))?;

    let build_resp = mikrom_proto::builder::BuildResponse::decode(&response.payload[..])
        .map_err(|e| ApiError::Internal(format!("Failed to decode builder response: {}", e)))?;

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
            None,
            None,
            None,
        )
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let task = crate::deploy::worker::BuildTask {
        deployment_id: deployment.id,
        app_id: app.id,
        app_name: app.name.clone(),
        user_id: auth.user_id.clone(),
        build_id: build_id.clone(),
        vcpus,
        memory_mib: memory_mib as u64,
        disk_mib: disk_mib as u64,
        port: app.port as u32,
        env: env_vars,
    };

    crate::deploy::worker::start_build_polling(state.clone(), task).await;

    Ok(Json(crate::deploy::DeployResponseBody {
        job_id: None,
        deployment_id: Some(deployment.id.to_string()),
        status: "BUILDING".to_string(),
        host_id: None,
        vm_id: None,
        image_tag: None,
        message: "Build initiated via NATS".to_string(),
    }))
}

pub async fn trigger_app_build(
    state: crate::AppState,
    app: crate::models::app::App,
) -> ApiResult<Uuid> {
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
            trigger_source: "github_webhook".to_string(),
        })
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    state
        .app_repo
        .update_deployment_status(
            deployment.id,
            "BUILDING",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    // Notify cluster via NATS for BUILDING phase
    {
        use mikrom_proto::scheduler::AppInfo;
        use prost::Message;
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
        let mut buf = Vec::new();
        if info.encode(&mut buf).is_ok() {
            let _ = state
                .nats_client
                .publish("mikrom.scheduler.job_updates", buf.into())
                .await;
        }
    }

    state.deployment_events.send(app.id).ok();

    use prost::Message;
    let build_req = mikrom_proto::builder::BuildRequest {
        app_id: app.id.to_string(),
        git_url: app.git_url.clone(),
        image_name: app.name.to_lowercase().replace(' ', "-"),
        tag: deployment.id.to_string(),
    };

    let mut buf = Vec::new();
    build_req
        .encode(&mut buf)
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let response = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        state
            .nats_client
            .request("mikrom.builder.build", buf.into()),
    )
    .await
    .map_err(|_| ApiError::Internal("Builder request timed out".into()))?
    .map_err(|e| ApiError::Internal(format!("Failed to trigger build via NATS: {}", e)))?;

    let build_resp = mikrom_proto::builder::BuildResponse::decode(&response.payload[..])
        .map_err(|e| ApiError::Internal(format!("Failed to decode builder response: {}", e)))?;

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
            None,
            None,
            None,
        )
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let task = crate::deploy::worker::BuildTask {
        deployment_id: deployment.id,
        app_id: app.id,
        app_name: app.name.clone(),
        user_id: app.user_id.to_string(),
        build_id: build_id.clone(),
        vcpus: vcpus as u32,
        memory_mib: memory_mib as u64,
        disk_mib: disk_mib as u64,
        port: app.port as u32,
        env: env_vars,
    };

    crate::deploy::worker::start_build_polling(state.clone(), task).await;

    Ok(deployment.id)
}

async fn get_app_by_name_and_auth(
    state: &AppState,
    app_name: &str,
    auth: &AuthUser,
) -> ApiResult<crate::models::app::App> {
    let app = state
        .app_repo
        .get_app_by_name(app_name)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or_else(|| ApiError::NotFound("Application not found".into()))?;

    if app.user_id.to_string() != auth.user_id {
        return Err(ApiError::Forbidden);
    }

    Ok(app)
}
