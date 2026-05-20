use crate::AppState;
use crate::deploy::worker::{BuildTask, start_build_polling};
use crate::deploy::workflow::DeploymentPromotionWorkflow;
use crate::error::{ApiError, ApiResult};
use crate::models::app::{App, Deployment};
use crate::repositories::app_repository::UpdateDeploymentParams;
use crate::workspace::{WorkspaceEvent, WorkspaceEventKind};
use mikrom_proto::scheduler::{AppConfig, DeployRequest, DeployResponse};

pub struct DeploymentService;

const DEFAULT_ZERO_DOWNTIME_HEALTH_CHECK_MAX_ATTEMPTS: usize = 45;
const DEFAULT_ZERO_DOWNTIME_HEALTH_CHECK_REQUEST_TIMEOUT_SECS: u64 = 2;
pub(crate) const ZERO_DOWNTIME_HEALTH_CHECK_RETRY_DELAY_SECS: u64 = 1;

pub struct DeployParams {
    pub image_tag: String,
    pub vcpus: u32,
    pub memory_mib: u32,
    pub disk_mib: u32,
    pub port: u32,
    pub env: std::collections::HashMap<String, String>,
}

impl DeploymentService {
    fn parse_usize_env(value: Option<String>, default: usize) -> usize {
        value
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(default)
    }

    fn parse_u64_env(value: Option<String>, default: u64) -> u64 {
        value
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(default)
    }

    pub(crate) fn zero_downtime_health_check_max_attempts() -> usize {
        Self::parse_usize_env(
            std::env::var("MIKROM_ZERO_DOWNTIME_HEALTH_CHECK_MAX_ATTEMPTS").ok(),
            DEFAULT_ZERO_DOWNTIME_HEALTH_CHECK_MAX_ATTEMPTS,
        )
    }

    pub(crate) fn zero_downtime_health_check_request_timeout() -> std::time::Duration {
        let secs = Self::parse_u64_env(
            std::env::var("MIKROM_ZERO_DOWNTIME_HEALTH_CHECK_TIMEOUT_SECS").ok(),
            DEFAULT_ZERO_DOWNTIME_HEALTH_CHECK_REQUEST_TIMEOUT_SECS,
        );

        std::time::Duration::from_secs(secs)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn trigger_build(
        state: &AppState,
        app: &App,
        deployment: &Deployment,
        vcpus: u32,
        memory_mib: u64,
        disk_mib: u64,
        env: std::collections::HashMap<String, String>,
        guard: crate::DeploymentFlowGuard,
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
        state.publish_workspace_event(WorkspaceEvent {
            kind: WorkspaceEventKind::DeploymentChanged,
            user_id: Some(app.user_id),
            app_id: Some(app.id),
            app_name: Some(app.name.clone()),
            deployment_id: Some(deployment.id),
            volume_id: None,
            resource_id: Some(deployment.id.to_string()),
        });

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

        start_build_polling(state.clone(), task, Some(guard)).await;

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

        let user = state
            .user_repo
            .find_by_id(app.user_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

        let vpc_ipv6_prefix = user.vpc_ipv6_prefix.unwrap_or_default();

        let volumes = state
            .volume_repo
            .list_volumes_by_app(app.id)
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
                port: params.port,
                env: params.env,
                health_check_path: app.health_check_path.clone(),
                volumes: volumes
                    .into_iter()
                    .map(|v| mikrom_proto::scheduler::Volume {
                        volume_id: v.id.to_string(),
                        size_mib: v.size_mib as u64,
                        read_only: false, // Default to RW
                        pool_name: v.pool_name,
                        mount_point: v.mount_point,
                        access_mode: v.access_mode,
                    })
                    .collect(),
                ..Default::default()
            }),
            deployment_id: deployment.id.to_string(),
            vpc_ipv6_prefix,
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
                let error_text = e.to_string();
                let scheduler_unavailable = error_text.contains("no responders");

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
                state.publish_workspace_event(WorkspaceEvent {
                    kind: WorkspaceEventKind::DeploymentChanged,
                    user_id: Some(app.user_id),
                    app_id: Some(app.id),
                    app_name: Some(app.name.clone()),
                    deployment_id: Some(deployment.id),
                    volume_id: None,
                    resource_id: Some(deployment.id.to_string()),
                });
                if scheduler_unavailable {
                    return Err(ApiError::Scheduler(
                        "Scheduler is not available right now".to_string(),
                    ));
                }

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
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        state.deployment_events.send(app.id).ok();
        state.publish_workspace_event(WorkspaceEvent {
            kind: WorkspaceEventKind::DeploymentChanged,
            user_id: Some(app.user_id),
            app_id: Some(app.id),
            app_name: Some(app.name.clone()),
            deployment_id: Some(deployment.id),
            volume_id: None,
            resource_id: Some(deployment.id.to_string()),
        });

        Ok(inner)
    }

    pub fn run_zero_downtime_flow(
        state: crate::AppState,
        app: crate::models::app::App,
        deployment: crate::models::app::Deployment,
        inner: mikrom_proto::scheduler::DeployResponse,
        user_id: String,
        cleanup_on_failure: bool,
        _guard: crate::DeploymentFlowGuard,
    ) {
        DeploymentPromotionWorkflow::run_zero_downtime_flow(
            state,
            app,
            deployment,
            inner,
            user_id,
            cleanup_on_failure,
            _guard,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::app::{App, Deployment};
    use crate::nats::{NatsClient, TypedNatsClient};
    use crate::repositories::app_repository::MockAppRepository;
    use crate::repositories::user_repository::{MockUserRepository, User, UserRole};
    use crate::repositories::volume_repository::MockVolumeRepository;
    use async_trait::async_trait;
    use sqlx::types::Uuid;
    use std::sync::Arc;

    struct NoRespondersNatsClient;

    #[async_trait]
    impl NatsClient for NoRespondersNatsClient {
        async fn request_raw(
            &self,
            _subject: String,
            _payload: Vec<u8>,
        ) -> anyhow::Result<Vec<u8>> {
            Err(anyhow::anyhow!(
                "NATS request failed: no responders: no responders"
            ))
        }

        async fn publish_raw(&self, _subject: String, _payload: Vec<u8>) -> anyhow::Result<()> {
            Ok(())
        }

        async fn subscribe_raw(&self, _subject: String) -> anyhow::Result<async_nats::Subscriber> {
            Err(anyhow::anyhow!("not used"))
        }
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn zero_downtime_defaults_cover_init_window() {
        assert!(
            DEFAULT_ZERO_DOWNTIME_HEALTH_CHECK_MAX_ATTEMPTS >= 35,
            "Default zero-downtime health-check attempts should exceed mikrom-init's 30s wait"
        );
        assert_eq!(DEFAULT_ZERO_DOWNTIME_HEALTH_CHECK_REQUEST_TIMEOUT_SECS, 2);
    }

    #[test]
    fn zero_downtime_env_parsing_falls_back_on_invalid_values() {
        assert_eq!(DeploymentService::parse_usize_env(None, 9), 9);
        assert_eq!(
            DeploymentService::parse_usize_env(Some("7".to_string()), 9),
            7
        );
        assert_eq!(
            DeploymentService::parse_usize_env(Some("not-a-number".to_string()), 9),
            9
        );

        assert_eq!(DeploymentService::parse_u64_env(None, 11), 11);
        assert_eq!(
            DeploymentService::parse_u64_env(Some("13".to_string()), 11),
            13
        );
        assert_eq!(
            DeploymentService::parse_u64_env(Some("bad".to_string()), 11),
            11
        );
    }

    #[tokio::test]
    async fn deploy_to_scheduler_maps_no_responders_to_scheduler_unavailable() {
        let app_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let deployment_id = Uuid::new_v4();

        let mut app_repo = MockAppRepository::new();
        app_repo
            .expect_update_deployment()
            .times(2)
            .returning(|_, _| Ok(()));

        let mut user_repo = MockUserRepository::new();
        user_repo.expect_find_by_id().returning(move |_| {
            Ok(Some(User {
                id: user_id,
                email: "test@example.com".to_string(),
                password_hash: "hash".to_string(),
                role: UserRole::User,
                first_name: None,
                last_name: None,
                vpc_ipv6_prefix: Some("fd00::".to_string()),
            }))
        });

        let mut volume_repo = MockVolumeRepository::new();
        volume_repo
            .expect_list_volumes_by_app()
            .times(1)
            .returning(|_| Ok(vec![]));

        let state = crate::AppState {
            user_repo: Arc::new(user_repo),
            app_repo: Arc::new(app_repo),
            github_repo: Arc::new(crate::repositories::MockGithubRepository::default()),
            volume_repo: Arc::new(volume_repo),
            scheduler: Arc::new(crate::scheduler::MockScheduler::new()),
            nats: TypedNatsClient::new_custom(Arc::new(NoRespondersNatsClient)),
            router_addr: "http://localhost:8080".to_string(),
            frontend_url: "http://localhost:3000".to_string(),
            api_db: sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap(),
            jwt_secret: "secret".into(),
            master_key: "key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            workspace_events: tokio::sync::broadcast::channel(1).0,
            mesh_status: tokio::sync::watch::channel(crate::vms::MeshStatus::default()).0,
            acme_email: "admin@example.com".into(),
            acme_staging: true,
            acme_check_interval: 3600,
            github_app_id: None,
            github_private_key: None,
            github_app_slug: None,
            github_webhook_url_base: None,
            active_deployment_flows: Arc::new(dashmap::DashSet::new()),
        };

        let app = App {
            id: app_id,
            user_id,
            name: "demo".into(),
            git_url: "https://example.com/demo".into(),
            port: 8080,
            hostname: Some("demo.example.com".into()),
            ..Default::default()
        };

        let deployment = Deployment {
            id: deployment_id,
            app_id,
            user_id,
            status: "PENDING".into(),
            ..Default::default()
        };

        let err = DeploymentService::deploy_to_scheduler(
            &state,
            &app,
            &deployment,
            DeployParams {
                image_tag: "image:tag".into(),
                vcpus: 1,
                memory_mib: 256,
                disk_mib: 512,
                port: 8080,
                env: std::collections::HashMap::new(),
            },
        )
        .await
        .expect_err("no responders should map to scheduler unavailable");

        assert!(matches!(err, ApiError::Scheduler(_)));
    }
}
