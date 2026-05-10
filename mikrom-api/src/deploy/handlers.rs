use crate::AppState;
use crate::auth::AuthUser;
use crate::deploy::service::{DeployParams, DeploymentService};
use crate::error::{ApiError, ApiResult};
use crate::models::app::Deployment;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::sse::{Event, Sse},
};
use futures::stream::Stream;
use rand::distr::{Alphanumeric, SampleString};
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
    pub github_installation_id: Option<i64>,
    pub github_repo_id: Option<i64>,
    pub github_repo_full_name: Option<String>,
    pub health_check_path: Option<String>,
    pub drain_timeout: Option<i32>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AppSecretResponse {
    pub github_webhook_secret: Option<String>,
}

#[utoipa::path(
    get,
    path = "/v1/apps/{app_name}/secret",
    params(
        ("app_name" = String, Path, description = "Application name")
    ),
    responses(
        (status = 200, description = "Get application secret", body = AppSecretResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Application not found", body = crate::error::ErrorResponse)
    ),
    tag = "apps",
    security(
        ("jwt" = [])
    )
)]
pub async fn get_app_secret_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(app_name): Path<String>,
) -> ApiResult<Json<AppSecretResponse>> {
    let app = get_app_by_name_and_auth(&state, &app_name, &auth).await?;

    Ok(Json(AppSecretResponse {
        github_webhook_secret: app.github_webhook_secret,
    }))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AppResponse {
    pub id: Uuid,
    pub name: String,
    pub git_url: String,
    pub port: u32,
    pub hostname: Option<String>,
    pub github_webhook_secret: Option<String>,
    pub github_installation_id: Option<i64>,
    pub github_repo_id: Option<i64>,
    pub github_repo_full_name: Option<String>,
    pub active_deployment_id: Option<Uuid>,
    pub health_check_path: String,
    pub drain_timeout: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ManualDeployRequest {
    pub vcpus: Option<u32>,
    pub memory_mib: Option<u32>,
    pub disk_mib: Option<u32>,
    pub env: Option<std::collections::HashMap<String, String>>,
    pub image: Option<String>,
}

#[utoipa::path(
    post,
    path = "/v1/apps",
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
        "{}.apps.mikrom.spluca.org",
        payload.name.to_lowercase().replace(' ', "-")
    );

    let webhook_secret = Alphanumeric.sample_string(&mut rand::rng(), 32);

    let user_id =
        uuid::Uuid::parse_str(&auth.user_id).map_err(|e| ApiError::Internal(e.to_string()))?;

    let app = state
        .app_repo
        .create_app(crate::repositories::app_repository::CreateAppParams {
            name: payload.name.clone(),
            git_url: payload.git_url,
            port: port as i32,
            hostname: Some(hostname),
            user_id,
            github_webhook_secret: Some(webhook_secret.clone()),
            github_installation_id: payload.github_installation_id,
            github_repo_id: payload.github_repo_id,
            github_repo_full_name: payload.github_repo_full_name.clone(),
            health_check_path: payload.health_check_path,
            drain_timeout: payload.drain_timeout,
        })
        .await?;

    // Automatically create GitHub Webhook if repo is connected
    tracing::info!(
        app_name = %app.name,
        has_inst = app.github_installation_id.is_some(),
        has_repo = app.github_repo_full_name.is_some(),
        has_app_id = state.github_app_id.is_some(),
        has_key = state.github_private_key.is_some(),
        "Checking if automatic GitHub webhook should be created"
    );

    if let Some(installation_id) = app.github_installation_id
        && let Some(repo_full_name) = &app.github_repo_full_name
        && let Some(github_app_id) = &state.github_app_id
        && let Some(github_private_key) = &state.github_private_key
    {
        tracing::info!(app_name = %app.name, "Initiating automatic GitHub webhook creation");
        let webhook_url = if let Some(base) = &state.github_webhook_url_base {
            if base.contains("smee.io") {
                // Smee.io doesn't support subpaths on the public URL
                base.to_string()
            } else {
                format!(
                    "{}/v1/webhooks/github/{}",
                    base.trim_end_matches('/'),
                    app.name
                )
            }
        } else {
            // Fallback: Try to guess the API URL from the frontend URL
            let url = format!(
                "{}/v1/webhooks/github/{}",
                state.frontend_url.replace("3000", "5001"),
                app.name
            );
            tracing::warn!(
                app_name = %app.name,
                %url,
                "GITHUB_WEBHOOK_URL_BASE is missing, using fragile fallback URL guessing"
            );
            url
        };

        // Use a background task to not block the response
        let github_app_id = github_app_id.clone();
        let github_private_key = github_private_key.clone();
        let repo_full_name = repo_full_name.clone();
        let webhook_secret = webhook_secret.clone();

        let app_id = app.id;
        tokio::spawn(async move {
            if let Err(e) = crate::github::create_repository_webhook(
                &github_app_id,
                &github_private_key,
                installation_id,
                &repo_full_name,
                &webhook_url,
                &webhook_secret,
            )
            .await
            {
                tracing::error!(%app_id, error = %e, "Failed to automatically create GitHub webhook");
            } else {
                tracing::info!(app_name = %payload.name, "Successfully created automatic GitHub webhook");
            }
        });
    }

    Ok((
        StatusCode::CREATED,
        Json(AppResponse {
            id: app.id,
            name: app.name,
            git_url: app.git_url,
            port: app.port as u32,
            hostname: app.hostname,
            github_webhook_secret: app.github_webhook_secret,
            github_installation_id: app.github_installation_id,
            github_repo_id: app.github_repo_id,
            github_repo_full_name: app.github_repo_full_name,
            active_deployment_id: app.active_deployment_id,
            health_check_path: app.health_check_path,
            drain_timeout: app.drain_timeout,
            created_at: app.created_at,
        }),
    ))
}

#[utoipa::path(
    get,
    path = "/v1/apps",
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
    let user_id =
        uuid::Uuid::parse_str(&auth.user_id).map_err(|e| ApiError::Internal(e.to_string()))?;
    let apps = state
        .app_repo
        .list_apps_by_user(Some(user_id))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(Json(
        apps.into_iter()
            .map(|a| AppResponse {
                id: a.id,
                name: a.name,
                git_url: a.git_url,
                port: a.port as u32,
                hostname: a.hostname,
                github_webhook_secret: a
                    .github_webhook_secret
                    .as_ref()
                    .map(|_| "********".to_string()),
                github_installation_id: a.github_installation_id,
                github_repo_id: a.github_repo_id,
                github_repo_full_name: a.github_repo_full_name,
                active_deployment_id: a.active_deployment_id,
                health_check_path: a.health_check_path,
                drain_timeout: a.drain_timeout,
                created_at: a.created_at,
            })
            .collect(),
    ))
}

#[utoipa::path(
    delete,
    path = "/v1/apps/{app_name}",
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

    // Tell the scheduler to clean up ALL resources for this app
    state
        .scheduler
        .delete_all_by_app(app.id.to_string(), app.user_id.to_string())
        .await
        .map_err(|e| {
            ApiError::Internal(format!("Failed to clean up scheduler resources: {}", e))
        })?;

    #[allow(clippy::collapsible_if)]
    if let Some(hostname) = &app.hostname {
        if let Err(e) = state.remove_route(hostname).await {
            tracing::error!("Failed to remove route for app in router: {}", e);
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
    path = "/v1/apps/{app_name}/deployments",
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
    path = "/v1/apps/{app_name}/deployments/stream",
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
        .nats
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
    path = "/v1/apps/{app_name}/deployments/{deployment_id}/activate",
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
    Path((app_name, deployment_id)): Path<(String, Uuid)>,
) -> ApiResult<StatusCode> {
    let app = get_app_by_name_and_auth(&state, &app_name, &auth).await?;

    let deployment = state
        .app_repo
        .get_deployment(deployment_id)
        .await?
        .ok_or(ApiError::NotFound("Deployment not found".into()))?;

    if deployment.status == "FAILED" {
        return Err(ApiError::BadRequest(
            "Cannot activate a failed deployment".into(),
        ));
    }

    if deployment.app_id != app.id {
        return Err(ApiError::BadRequest(
            "Deployment does not belong to this application".into(),
        ));
    }

    // Protect against concurrent flows before any scheduler interaction
    let guard = state.try_start_flow(app.id.into()).ok_or_else(|| {
        ApiError::BadRequest("A deployment flow is already in progress for this application".into())
    })?;

    match deployment.job_id.clone() {
        Some(job_id) => {
            info!(job_id = %job_id, status = %deployment.status, "Activating deployment with zero-downtime flow...");

            use crate::deploy::service::{DeployParams, DeploymentService};
            use mikrom_proto::scheduler::{DeployResponse, DeployStatus};

            let (inner, cleanup_on_failure) =
                if deployment.status == "STOPPED" || deployment.status == "FAILED" {
                    info!(app = %app.name, "Deployment is not running, starting it first...");
                    let env_vars: std::collections::HashMap<String, String> =
                        serde_json::from_value(deployment.env_vars.clone()).unwrap_or_default();

                    let inner = match DeploymentService::deploy_to_scheduler(
                        &state,
                        &app,
                        &deployment,
                        DeployParams {
                            image_tag: deployment.image_tag.clone().unwrap_or_default(),
                            vcpus: deployment.vcpus as u32,
                            memory_mib: deployment.memory_mib as u32,
                            disk_mib: deployment.disk_mib as u32,
                            env: env_vars,
                        },
                    )
                    .await
                    {
                        Ok(inner) => inner,
                        Err(e) => {
                            return Err(e);
                        },
                    };
                    (inner, true) // Cleanup if it fails to start/be healthy now
                } else {
                    let inner = DeployResponse {
                        job_id,
                        status: DeployStatus::Running as i32,
                        host_id: String::new(),
                        vm_id: String::new(),
                        message: "Activating".to_string(),
                    };
                    (inner, false) // Don't cleanup if it was already running
                };

            DeploymentService::run_zero_downtime_flow(
                state.clone(),
                app,
                deployment,
                inner,
                auth.user_id,
                cleanup_on_failure,
                guard,
            );

            Ok(StatusCode::ACCEPTED)
        },
        None => {
            info!(app = %app.name, deployment_id = %deployment.id, "Activating deployment record only...");

            state
                .app_repo
                .set_active_deployment(app.id, deployment_id)
                .await
                .map_err(|e| ApiError::Internal(e.to_string()))?;

            // Update local app for notify_router
            let mut updated_app = app;
            updated_app.active_deployment_id = Some(deployment_id);

            let _ = state.notify_router(&updated_app).await;
            state.deployment_events.send(updated_app.id).ok();

            Ok(StatusCode::OK)
        },
    }
}

#[utoipa::path(
    post,
    path = "/v1/apps/{app_name}/deploy",
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
    let image = payload.image.clone();

    // Try to fetch latest git metadata if linked to GitHub
    let mut git_metadata = None;
    if let Some(installation_id) = app.github_installation_id
        && let Some(repo_full_name) = &app.github_repo_full_name
    {
        match (&state.github_app_id, &state.github_private_key) {
            (Some(github_app_id), Some(github_private_key)) => {
                // TODO: Fetch the repository's default branch or use a configured branch
                // instead of hardcoding main/master.
                // For now, we try main then master as a sensible default.
                match crate::github::get_repo_latest_commit(
                    github_app_id,
                    github_private_key,
                    installation_id,
                    repo_full_name,
                    "main",
                )
                .await
                {
                    Ok(meta) => git_metadata = Some(meta),
                    Err(_) => {
                        // Try master if main fails
                        if let Ok(meta) = crate::github::get_repo_latest_commit(
                            github_app_id,
                            github_private_key,
                            installation_id,
                            repo_full_name,
                            "master",
                        )
                        .await
                        {
                            git_metadata = Some(meta);
                        }
                    },
                }
            },
            _ => {
                tracing::warn!(app_id = %app.id, "GitHub linked but API credentials missing in state")
            },
        }
    }

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
            git_commit_hash: git_metadata
                .as_ref()
                .and_then(|m| m.git_commit_hash.clone()),
            git_commit_message: git_metadata
                .as_ref()
                .and_then(|m| m.git_commit_message.clone()),
            git_branch: git_metadata.as_ref().and_then(|m| m.git_branch.clone()),
        })
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    // Protect against concurrent flows
    let guard = state.try_start_flow(app.id.into()).ok_or_else(|| {
        ApiError::BadRequest("A deployment flow is already in progress for this application".into())
    })?;

    // If an image is provided directly, skip the build phase and deploy immediately
    if let Some(image_tag) = image {
        info!(app = %app.name, image = %image_tag, "Direct image deployment requested, skipping build");

        // 1. Trigger deployment
        let inner = match DeploymentService::deploy_to_scheduler(
            &state,
            &app,
            &deployment,
            DeployParams {
                image_tag: image_tag.clone(),
                vcpus,
                memory_mib,
                disk_mib,
                env: env_vars.clone(),
            },
        )
        .await
        {
            Ok(inner) => inner,
            Err(e) => {
                return Err(e);
            },
        };

        // 2. Start Zero-Downtime orchestration in background
        DeploymentService::run_zero_downtime_flow(
            state.clone(),
            app,
            deployment.clone(),
            inner.clone(),
            auth.user_id.clone(),
            true,
            guard,
        );

        return Ok(Json(crate::deploy::DeployResponseBody {
            job_id: Some(inner.job_id),
            deployment_id: Some(deployment.id.to_string()),
            status: "HEALTH_CHECKING".to_string(),
            host_id: Some(inner.host_id),
            vm_id: Some(inner.vm_id),
            image_tag: Some(image_tag),
            message: "Deployment triggered, health check in progress".to_string(),
        }));
    }

    // Default: Trigger build
    DeploymentService::trigger_build(
        &state,
        &app,
        &deployment,
        vcpus,
        memory_mib as u64,
        disk_mib as u64,
        env_vars,
        guard,
    )
    .await?;

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
    git_metadata: Option<crate::repositories::app_repository::GitMetadata>,
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
            git_commit_hash: git_metadata
                .as_ref()
                .and_then(|m| m.git_commit_hash.clone()),
            git_commit_message: git_metadata
                .as_ref()
                .and_then(|m| m.git_commit_message.clone()),
            git_branch: git_metadata.as_ref().and_then(|m| m.git_branch.clone()),
        })
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    // Protect against concurrent flows
    let guard = state.try_start_flow(app.id.into()).ok_or_else(|| {
        ApiError::BadRequest("A deployment flow is already in progress for this application".into())
    })?;

    DeploymentService::trigger_build(
        &state,
        &app,
        &deployment,
        vcpus as u32,
        memory_mib as u64,
        disk_mib as u64,
        env_vars,
        guard,
    )
    .await?;

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
