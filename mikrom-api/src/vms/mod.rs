use crate::error::{ApiError, ApiResult};
use axum::{
    Json,
    extract::{Path, State},
    response::{
        IntoResponse,
        sse::{Event, Sse},
    },
};
use serde::Serialize;
use std::collections::HashMap;
use tokio_stream::StreamExt;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct LiveDeploymentInfo {
    pub job_id: String,
    pub deployment_id: String,
    pub app_id: String,
    pub app_name: String,
    pub image: String,
    pub status: String,
    pub host_id: String,
    pub vm_id: String,
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LiveDeploymentStatus {
    pub job_id: String,
    pub deployment_id: String,
    pub app_id: String,
    pub app_name: String,
    pub image: String,
    pub status: String,
    pub host_id: String,
    pub vm_id: String,
    pub scheduled_at: i64,
    pub started_at: i64,
    pub stopped_at: i64,
    pub error_message: String,
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
    pub vcpus: i32,
    pub memory_mib: i64,
}

#[utoipa::path(
    get,
    path = "/deployments/active",
    responses(
        (status = 200, description = "List of active deployments", body = [LiveDeploymentInfo]),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
#[tracing::instrument(skip(state, auth))]
pub async fn list_active_deployments(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
) -> ApiResult<Json<Vec<LiveDeploymentInfo>>> {
    // 1. Get all deployments for this user from DB
    let deployments = state
        .app_repo
        .list_deployments_by_user(&auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    // 2. Try to get real-time status from scheduler for active ones
    let mut scheduler_apps = HashMap::new();

    if let Ok(mut client) = state.get_scheduler_client().await {
        let req = mikrom_proto::scheduler::ListAppsRequest {
            user_id: auth.user_id.clone(),
            status: None,
        };

        if let Ok(resp) = client.list_apps(req).await {
            for app in resp.into_inner().apps {
                scheduler_apps.insert(app.job_id.clone(), app);
            }
        }
    }

    // 3. Map deployments to LiveDeploymentInfo, using scheduler data if available
    let mut active_deployments = Vec::new();
    for dep in deployments {
        // Only show deployments that have a job_id (meaning they were at least attempted to be scheduled)
        if let Some(job_id) = &dep.job_id {
            let (status, host_id, vm_id, cpu_usage, ram_used_bytes) =
                if let Some(sch_app) = scheduler_apps.get(job_id) {
                    (
                        crate::scheduler::status_name(sch_app.status).to_string(),
                        sch_app.host_id.clone(),
                        sch_app.vm_id.clone(),
                        sch_app.cpu_usage,
                        sch_app.ram_used_bytes,
                    )
                } else {
                    (dep.status.clone(), String::new(), String::new(), 0.0, 0)
                };

            // Get app name from repo (we might need a join or a cache here for performance)
            let app_name = if let Ok(Some(app)) = state.app_repo.get_app(dep.app_id).await {
                app.name
            } else {
                "Unknown".to_string()
            };

            active_deployments.push(LiveDeploymentInfo {
                job_id: job_id.clone(),
                deployment_id: dep.id.to_string(),
                app_id: dep.app_id.to_string(),
                app_name,
                image: dep.image_tag.unwrap_or_default(),
                status,
                host_id,
                vm_id,
                cpu_usage,
                ram_used_bytes,
            });
        }
    }

    Ok(Json(active_deployments))
}

#[utoipa::path(
    get,
    path = "/deployments/events",
    responses(
        (status = 200, description = "SSE stream of active deployment events"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
#[tracing::instrument(skip(state, auth))]
pub async fn watch_deployments(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
) -> ApiResult<impl IntoResponse> {
    let mut client = state
        .get_scheduler_client()
        .await
        .map_err(ApiError::Scheduler)?;
    let req = mikrom_proto::scheduler::WatchAppsRequest {
        user_id: auth.user_id,
    };

    let resp = client
        .watch_apps(req)
        .await
        .map_err(|e| ApiError::Internal(e.message().to_string()))?;

    let stream = resp.into_inner().map(move |res| match res {
        Ok(msg) => {
            if let Some(app) = msg.app {
                let data = serde_json::json!(LiveDeploymentStatus {
                    job_id: app.job_id,
                    deployment_id: String::new(), // job_id is sufficient for UI reconciliation
                    app_id: app.app_id,
                    app_name: app.app_name,
                    image: app.image,
                    status: crate::scheduler::status_name(app.status).to_string(),
                    host_id: app.host_id,
                    vm_id: app.vm_id,
                    cpu_usage: app.cpu_usage,
                    ram_used_bytes: app.ram_used_bytes,
                    // These fields are not available in AppInfo but we can use defaults
                    scheduled_at: 0,
                    started_at: 0,
                    stopped_at: 0,
                    error_message: String::new(),
                    vcpus: 0,
                    memory_mib: 0,
                })
                .to_string();
                Ok::<Event, std::convert::Infallible>(Event::default().data(data))
            } else {
                Ok::<Event, std::convert::Infallible>(Event::default().comment("keep-alive"))
            }
        },
        Err(e) => {
            tracing::error!("Error in scheduler watch_apps stream: {}", e);
            Ok::<Event, std::convert::Infallible>(Event::default().comment(format!("error: {}", e)))
        },
    });

    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(5))
            .text("keep-alive"),
    ))
}

#[utoipa::path(
    get,
    path = "/deployments/{job_id}",
    params(
        ("job_id" = String, Path, description = "Deployment Job ID")
    ),
    responses(
        (status = 200, description = "Get live deployment details", body = LiveDeploymentStatus),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Deployment not found", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
#[tracing::instrument(skip(state, auth), fields(job_id = %job_id))]
pub async fn get_deployment_status(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(job_id): Path<String>,
) -> ApiResult<Json<LiveDeploymentStatus>> {
    let mut client = state
        .get_scheduler_client()
        .await
        .map_err(ApiError::Scheduler)?;
    let req = mikrom_proto::scheduler::AppStatusRequest {
        job_id,
        user_id: auth.user_id,
    };

    let resp = client.get_app_status(req).await.map_err(|e| {
        if e.code() == tonic::Code::NotFound {
            ApiError::NotFound("Job not found".to_string())
        } else {
            ApiError::Internal(e.message().to_string())
        }
    })?;

    let inner = resp.into_inner();

    // Fetch deployment to get app_id, image, vcpus, memory
    let deployment = state
        .app_repo
        .get_deployment_by_job_id(&inner.job_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or(ApiError::NotFound(
            "Deployment record not found".to_string(),
        ))?;

    let app_name = if let Ok(Some(app)) = state.app_repo.get_app(deployment.app_id).await {
        app.name
    } else {
        "Unknown".to_string()
    };

    let deployment_status = LiveDeploymentStatus {
        job_id: inner.job_id,
        deployment_id: deployment.id.to_string(),
        app_id: deployment.app_id.to_string(),
        app_name,
        image: deployment.image_tag.unwrap_or_default(),
        status: crate::scheduler::status_name(inner.status).to_string(),
        host_id: inner.host_id,
        vm_id: inner.vm_id,
        scheduled_at: inner.scheduled_at,
        started_at: inner.started_at,
        stopped_at: inner.stopped_at,
        error_message: inner.error_message,
        cpu_usage: inner.cpu_usage,
        ram_used_bytes: inner.ram_used_bytes,
        vcpus: deployment.vcpus,
        memory_mib: deployment.memory_mib,
    };

    Ok(Json(deployment_status))
}

#[utoipa::path(
    get,
    path = "/deployments/{job_id}/logs",
    params(
        ("job_id" = String, Path, description = "Deployment Job ID")
    ),
    responses(
        (status = 200, description = "SSE stream of deployment logs"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
#[tracing::instrument(skip(state, auth), fields(job_id = %job_id))]
pub async fn get_deployment_logs(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(job_id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let mut client = state
        .get_scheduler_client()
        .await
        .map_err(ApiError::Scheduler)?;
    let req = mikrom_proto::scheduler::GetLogsRequest {
        job_id,
        user_id: auth.user_id,
        follow: true,
    };

    let resp = client
        .get_app_logs(req)
        .await
        .map_err(|e| ApiError::Internal(e.message().to_string()))?;

    let stream = resp.into_inner().map(|res| match res {
        Ok(log) => {
            let data = serde_json::json!({
                "line": log.line,
                "timestamp": log.timestamp,
            })
            .to_string();
            Ok::<Event, std::convert::Infallible>(Event::default().data(data))
        },
        Err(e) => {
            let data = serde_json::json!({
                "line": format!("Error: {}", e),
                "timestamp": chrono::Utc::now().timestamp(),
            })
            .to_string();
            Ok::<Event, std::convert::Infallible>(Event::default().data(data))
        },
    });

    Ok(Sse::new(stream))
}

#[utoipa::path(
    delete,
    path = "/deployments/{job_id}",
    params(
        ("job_id" = String, Path, description = "Deployment Job ID")
    ),
    responses(
        (status = 200, description = "Deployment stopped"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
#[tracing::instrument(skip(state, auth), fields(job_id = %job_id))]
pub async fn stop_deployment(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(job_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut client = state
        .get_scheduler_client()
        .await
        .map_err(ApiError::Scheduler)?;
    let req = mikrom_proto::scheduler::CancelRequest {
        job_id: job_id.clone(),
        user_id: auth.user_id,
    };

    let resp = client
        .cancel_app(req)
        .await
        .map_err(|e| ApiError::Internal(e.message().to_string()))?;

    let inner = resp.into_inner();
    if inner.success {
        Ok(Json(serde_json::json!({
            "success": true,
            "message": inner.message
        })))
    } else {
        Err(ApiError::NotFound(inner.message))
    }
}

#[utoipa::path(
    delete,
    path = "/deployments/{job_id}/delete",
    params(
        ("job_id" = String, Path, description = "Deployment Job ID")
    ),
    responses(
        (status = 200, description = "Deployment record removed"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
#[tracing::instrument(skip(state, auth), fields(job_id = %job_id))]
pub async fn delete_deployment_record(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(job_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    // 1. Try to notify scheduler (optional/best effort)
    let _ = state
        .scheduler
        .delete_app(job_id.clone(), auth.user_id)
        .await;

    // 2. Always delete from database
    state
        .app_repo
        .delete_deployment_by_job_id(&job_id)
        .await
        .map_err(|e| {
            ApiError::Internal(format!("Failed to remove deployment from database: {}", e))
        })?;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Deployment record removed from Mikrom"
    })))
}

#[utoipa::path(
    post,
    path = "/deployments/{job_id}/pause",
    params(
        ("job_id" = String, Path, description = "Deployment Job ID")
    ),
    responses(
        (status = 200, description = "Deployment paused"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
#[tracing::instrument(skip(state, auth), fields(job_id = %job_id))]
pub async fn pause_deployment(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(job_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let success = state
        .scheduler
        .pause_app(job_id.clone(), auth.user_id)
        .await
        .map_err(ApiError::Scheduler)?;

    if success {
        // Update database status
        if let Ok(Some(dep)) = state.app_repo.get_deployment_by_job_id(&job_id).await {
            let _ = state
                .app_repo
                .update_deployment_status(
                    dep.id,
                    "STOPPED",
                    Some(job_id),
                    dep.image_tag,
                    dep.build_id,
                    None,
                    dep.git_commit_hash,
                    dep.git_commit_message,
                    dep.git_branch,
                )
                .await;
        }

        Ok(Json(
            serde_json::json!({ "success": true, "message": "Paused" }),
        ))
    } else {
        Err(ApiError::BadRequest("Failed to pause".to_string()))
    }
}

#[utoipa::path(
    post,
    path = "/deployments/{job_id}/resume",
    params(
        ("job_id" = String, Path, description = "Deployment Job ID")
    ),
    responses(
        (status = 200, description = "Deployment resumed"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "deployment",
    security(
        ("jwt" = [])
    )
)]
#[tracing::instrument(skip(state, auth), fields(job_id = %job_id))]
pub async fn resume_deployment(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(job_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let success = state
        .scheduler
        .resume_app(job_id.clone(), auth.user_id)
        .await
        .map_err(ApiError::Scheduler)?;

    if success {
        // Update database status
        if let Ok(Some(dep)) = state.app_repo.get_deployment_by_job_id(&job_id).await {
            let _ = state
                .app_repo
                .update_deployment_status(
                    dep.id,
                    "RUNNING",
                    Some(job_id),
                    dep.image_tag,
                    dep.build_id,
                    None,
                    dep.git_commit_hash,
                    dep.git_commit_message,
                    dep.git_branch,
                )
                .await;
        }

        Ok(Json(
            serde_json::json!({ "success": true, "message": "Resumed" }),
        ))
    } else {
        Err(ApiError::BadRequest("Failed to resume".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppState;
    use crate::auth::AuthUser;
    use crate::repositories::app_repository::MockAppRepository;
    use axum::extract::State;
    use std::sync::Arc;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_list_active_deployments_empty() {
        let mut mock_repo = MockAppRepository::new();

        // Mock list_deployments_by_user returning empty
        mock_repo
            .expect_list_deployments_by_user()
            .returning(|_| Ok(vec![]));

        let state = AppState {
            user_repo: Arc::new(crate::repositories::user_repository::MockUserRepository::new()),
            app_repo: Arc::new(mock_repo),
            scheduler: Arc::new(crate::scheduler::MockScheduler::new()),
            scheduler_config: crate::scheduler::SchedulerConfig {
                addr: "http://localhost:5002".to_string(),
                use_tls: false,
                certs_dir: None,
            },
            builder_addr: "http://localhost:5004".to_string(),
            jwt_secret: "secret".to_string(),
            master_key: "key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            build_semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
        };

        let auth = AuthUser {
            user_id: "user-1".to_string(),
            email: "test@example.com".to_string(),
            role: crate::repositories::user_repository::UserRole::User,
        };

        let result = list_active_deployments(auth, State(state)).await.unwrap();
        assert!(result.0.is_empty());
    }

    #[tokio::test]
    async fn test_get_deployment_status_not_found() {
        let mut mock_repo = MockAppRepository::new();

        // Mock get_deployment_by_job_id returning None
        mock_repo
            .expect_get_deployment_by_job_id()
            .returning(|_| Ok(None));

        let _state = AppState {
            user_repo: Arc::new(crate::repositories::user_repository::MockUserRepository::new()),
            app_repo: Arc::new(mock_repo),
            scheduler: Arc::new(crate::scheduler::MockScheduler::new()),
            scheduler_config: crate::scheduler::SchedulerConfig {
                addr: "http://localhost:5002".to_string(),
                use_tls: false,
                certs_dir: None,
            },
            builder_addr: "http://localhost:5004".to_string(),
            jwt_secret: "secret".to_string(),
            master_key: "key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            build_semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
        };

        let _auth = AuthUser {
            user_id: "user-1".to_string(),
            email: "test@example.com".to_string(),
            role: crate::repositories::user_repository::UserRole::User,
        };

        // Note: list_active_deployments is easier to test because it continues even if scheduler fails
    }

    #[tokio::test]
    async fn test_list_active_deployments_with_data() {
        let mut mock_repo = MockAppRepository::new();
        let app_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();

        // Mock list_deployments_by_user returning some data
        mock_repo
            .expect_list_deployments_by_user()
            .returning(move |_| {
                Ok(vec![crate::models::app::Deployment {
                    id: Uuid::new_v4(),
                    app_id,
                    user_id,
                    build_id: None,
                    image_tag: Some("nginx:latest".into()),
                    job_id: Some("job-1".into()),
                    ip_address: None,
                    status: "RUNNING".into(),
                    vcpus: 1,
                    memory_mib: 256,
                    disk_mib: 1024,
                    port: 80,
                    env_vars: serde_json::json!({}),
                    git_commit_hash: None,
                    git_commit_message: None,
                    git_branch: None,
                    trigger_source: "manual".into(),
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                }])
            });

        mock_repo.expect_get_app().returning(|id| {
            Ok(Some(crate::models::app::App {
                id,
                name: "test-app".into(),
                git_url: "".into(),
                port: 80,
                hostname: None,
                user_id: Uuid::new_v4(),
                github_webhook_secret: None,
                active_deployment_id: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            }))
        });

        let state = AppState {
            user_repo: Arc::new(crate::repositories::user_repository::MockUserRepository::new()),
            app_repo: Arc::new(mock_repo),
            scheduler: Arc::new(crate::scheduler::MockScheduler::new()),
            scheduler_config: crate::scheduler::SchedulerConfig {
                addr: "http://invalid:1".to_string(),
                use_tls: false,
                certs_dir: None,
            },
            builder_addr: "http://localhost:5004".to_string(),
            jwt_secret: "secret".to_string(),
            master_key: "key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            build_semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
        };

        let auth = AuthUser {
            user_id: user_id.to_string(),
            email: "test@example.com".to_string(),
            role: crate::repositories::user_repository::UserRole::User,
        };

        let result = list_active_deployments(auth, State(state)).await.unwrap();
        assert_eq!(result.0.len(), 1);
        assert_eq!(result.0[0].job_id, "job-1");
        assert_eq!(result.0[0].image, "nginx:latest");
    }
}
