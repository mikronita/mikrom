use crate::AppState;
use crate::domain::{App, UpdateDeploymentParams};
use crate::workspace::{WorkspaceEvent, WorkspaceEventKind};
use uuid::Uuid;

pub struct DeploymentOrchestrator;

impl DeploymentOrchestrator {
    pub async fn promote_deployment_to_active(
        state: &AppState,
        app: App,
        deployment_id: Uuid,
    ) -> anyhow::Result<(App, Option<Uuid>)> {
        let previous_active_id = app.active_deployment_id;
        state
            .app_repo
            .set_active_deployment(app.id, deployment_id)
            .await?;

        let mut updated_app = app;
        updated_app.active_deployment_id = Some(deployment_id);
        state.notify_router(&updated_app).await?;
        state.deployment_events.send(updated_app.id).ok();
        state.publish_workspace_event(WorkspaceEvent {
            kind: WorkspaceEventKind::DeploymentChanged,
            tenant_id: Some(updated_app.tenant_id),
            user_id: None,
            app_id: Some(updated_app.id),
            app_name: Some(updated_app.name.clone()),
            deployment_id: Some(deployment_id),
            volume_id: None,
            resource_id: Some(deployment_id.to_string()),
        });

        Ok((updated_app, previous_active_id))
    }

    pub async fn restore_previous_deployment_to_running(
        state: &AppState,
        app_id: Uuid,
        previous_active_id: Option<Uuid>,
    ) -> anyhow::Result<()> {
        if let Some(old_id) = previous_active_id {
            state
                .app_repo
                .update_deployment(
                    old_id,
                    UpdateDeploymentParams {
                        status: Some("RUNNING".to_string()),
                        ..Default::default()
                    },
                )
                .await?;
            state.deployment_events.send(app_id).ok();
            if let Ok(Some(app)) = state.app_repo.get_app(app_id).await {
                state.publish_workspace_event(WorkspaceEvent {
                    kind: WorkspaceEventKind::DeploymentChanged,
                    tenant_id: Some(app.tenant_id),
                    user_id: None,
                    app_id: Some(app.id),
                    app_name: Some(app.name),
                    deployment_id: Some(old_id),
                    volume_id: None,
                    resource_id: Some(old_id.to_string()),
                });
            }
        }

        Ok(())
    }

    pub async fn drain_previous_deployment_after_promotion(
        state: &AppState,
        app_name: &str,
        previous_active_id: Option<Uuid>,
    ) -> anyhow::Result<()> {
        if let Some(old_id) = previous_active_id
            && let Some(old_dep) = state.app_repo.get_deployment(old_id).await?
        {
            let app = state
                .app_repo
                .get_app(old_dep.app_id)
                .await?
                .ok_or_else(|| {
                    anyhow::anyhow!("Application missing while draining previous deployment")
                })?;

            if let Some(old_job_id) = old_dep.job_id.clone() {
                let job_id = old_job_id.clone();
                tracing::info!(
                    app = %app_name,
                    job_id = %old_job_id,
                    deployment_id = %old_id,
                    "Stopping old version immediately after promotion"
                );
                tracing::info!(
                    app = %app_name,
                    job_id = %job_id,
                    deployment_id = %old_id,
                    origin = "zero_downtime_drain",
                    user_id = "system",
                    "Forwarding pause request to scheduler"
                );
                state
                    .scheduler
                    .pause_app(job_id.clone(), "system".to_string())
                    .await
                    .map_err(|e| anyhow::anyhow!(e))?;
                tracing::info!(
                    app = %app_name,
                    job_id = %job_id,
                    deployment_id = %old_id,
                    origin = "zero_downtime_drain",
                    user_id = "system",
                    "Scheduler pause completed"
                );
                state
                    .app_repo
                    .update_deployment(
                        old_id,
                        UpdateDeploymentParams {
                            status: Some("PAUSED".to_string()),
                            ..Default::default()
                        },
                    )
                    .await?;
            }

            state.publish_workspace_event(WorkspaceEvent {
                kind: WorkspaceEventKind::DeploymentChanged,
                tenant_id: Some(app.tenant_id),
                user_id: None,
                app_id: Some(app.id),
                app_name: Some(app_name.to_string()),
                deployment_id: Some(old_id),
                volume_id: None,
                resource_id: Some(old_id.to_string()),
            });
        }

        Ok(())
    }

    pub async fn mark_previous_deployment_draining(
        state: &AppState,
        app_name: &str,
        app_id: Uuid,
        previous_active_id: Option<Uuid>,
    ) -> anyhow::Result<()> {
        if let Some(old_id) = previous_active_id
            && let Some(old_dep) = state.app_repo.get_deployment(old_id).await?
        {
            state
                .app_repo
                .update_deployment(
                    old_id,
                    UpdateDeploymentParams {
                        status: Some("DRAINING".to_string()),
                        ..Default::default()
                    },
                )
                .await?;
            state.deployment_events.send(app_id).ok();
            state.publish_workspace_event(WorkspaceEvent {
                kind: WorkspaceEventKind::DeploymentChanged,
                tenant_id: Some(old_dep.tenant_id),
                user_id: None,
                app_id: Some(old_dep.app_id),
                app_name: Some(app_name.to_string()),
                deployment_id: Some(old_id),
                volume_id: None,
                resource_id: Some(old_id.to_string()),
            });
            if let Some(old_job_id) = old_dep.job_id {
                tracing::info!(
                    app = %app_name,
                    job_id = %old_job_id,
                    deployment_id = %old_id,
                    origin = "zero_downtime_drain",
                    user_id = "system",
                    "Marked previous deployment as draining"
                );
            } else {
                tracing::info!(
                    app = %app_name,
                    deployment_id = %old_id,
                    origin = "zero_downtime_drain",
                    user_id = "system",
                    "Marked previous deployment as draining"
                );
            }
        }

        Ok(())
    }

    pub async fn rollback_failed_promotion(
        state: &AppState,
        app_name: &str,
        app_id: Uuid,
        deployment_id: Uuid,
        job_id: &str,
        previous_active_id: Option<Uuid>,
    ) -> anyhow::Result<()> {
        if let Some(old_id) = previous_active_id {
            state
                .app_repo
                .update_deployment(
                    old_id,
                    UpdateDeploymentParams {
                        status: Some("RUNNING".to_string()),
                        ..Default::default()
                    },
                )
                .await?;
        }

        tracing::info!(
            app = %app_name,
            job_id = %job_id,
            deployment_id = %deployment_id,
            origin = "zero_downtime_cleanup",
            user_id = "system",
            "Forwarding pause request to scheduler"
        );
        state
            .scheduler
            .pause_app(job_id.to_string(), "system".to_string())
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        tracing::info!(
            app = %app_name,
            job_id = %job_id,
            deployment_id = %deployment_id,
            origin = "zero_downtime_cleanup",
            user_id = "system",
            "Scheduler pause completed"
        );
        state
            .app_repo
            .update_deployment(
                deployment_id,
                UpdateDeploymentParams {
                    status: Some("FAILED".to_string()),
                    ..Default::default()
                },
            )
            .await?;
        state.deployment_events.send(app_id).ok();
        state.publish_workspace_event(WorkspaceEvent {
            kind: WorkspaceEventKind::DeploymentChanged,
            user_id: None,
            tenant_id: state
                .app_repo
                .get_app(app_id)
                .await
                .ok()
                .flatten()
                .map(|app| app.tenant_id),
            app_id: Some(app_id),
            app_name: Some(app_name.to_string()),
            deployment_id: Some(deployment_id),
            volume_id: None,
            resource_id: Some(job_id.to_string()),
        });

        Ok(())
    }
}

#[cfg(any())]
mod tests {
    use super::*;
    use crate::domain::github::MockGithubRepository;
    use crate::domain::{
        Deployment, MockAppRepository, MockDatabaseRepository, MockUserRepository,
    };
    use mockall::predicate::{eq, function};
    use std::sync::Arc;
    use uuid::Uuid;

    async fn connect_nats_or_skip(test_name: &str) -> Option<async_nats::Client> {
        let nats_url =
            std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());

        match async_nats::connect(nats_url).await {
            Ok(client) => Some(client),
            Err(err) => {
                eprintln!("skipping {}: unable to connect to NATS: {}", test_name, err);
                None
            },
        }
    }

    #[tokio::test]
    async fn restore_previous_deployment_marks_old_deployment_running() {
        let Some(nats_client) =
            connect_nats_or_skip("restore_previous_deployment_marks_old_deployment_running").await
        else {
            return;
        };

        let mut mock_app_repo = MockAppRepository::new();
        let old_dep_id = Uuid::new_v4();
        let app_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();

        mock_app_repo
            .expect_update_deployment()
            .with(
                eq(old_dep_id),
                function(|params: &crate::domain::UpdateDeploymentParams| {
                    params.status == Some("RUNNING".to_string())
                }),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        mock_app_repo
            .expect_get_app()
            .with(eq(app_id))
            .times(1)
            .returning(move |_| {
                Ok(Some(App {
                    id: app_id,
                    tenant_id: user_id,
                    name: "test-app".to_string(),
                    ..Default::default()
                }))
            });

        let user_repo = Arc::new(MockUserRepository::new());
        let app_repo = Arc::new(mock_app_repo);
        let github_repo = Arc::new(MockGithubRepository::default());
        let volume_repo = Arc::new(crate::domain::MockVolumeRepository::new());
        let scheduler = Arc::new(crate::domain::MockScheduler::new());
        let nats = crate::nats::TypedNatsClient::new(nats_client);
        let db = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();

        let ctx = crate::application::ApiContext {
            user_repo: user_repo.clone(),
            database_repo: Arc::new(MockDatabaseRepository::new()),
            app_repo: app_repo.clone(),
            github_repo: github_repo.clone(),
            volume_repo: volume_repo.clone(),
            scheduler: scheduler.clone(),
            nats: nats.clone(),
            db: db.clone(),
            config: Arc::new(crate::config::ApiConfig::default()),
            jwt_secret: "secret".to_string(),
            master_key: "key".to_string(),
        };

        let state = AppState {
            ctx,
            user_repo,
            database_repo: Arc::new(MockDatabaseRepository::new()),
            app_repo,
            github_repo,
            volume_repo,
            scheduler,
            nats,
            router_addr: "http://localhost:8080".to_string(),
            frontend_url: "http://localhost:3000".to_string(),
            api_db: db,
            jwt_secret: "secret".to_string(),
            master_key: "key".to_string(),
            deployment_events: tokio::sync::broadcast::channel(100).0,
            workspace_events: tokio::sync::broadcast::channel(100).0,
            mesh_status:
                tokio::sync::watch::channel(crate::application::vms::MeshStatus::default()).0,
            acme_email: "test@example.com".to_string(),
            acme_staging: true,
            acme_check_interval: 3600,
            github_app_id: None,
            github_private_key: None,
            github_app_slug: None,
            github_webhook_url_base: None,
            active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
        };

        DeploymentOrchestrator::restore_previous_deployment_to_running(
            &state,
            app_id,
            Some(old_dep_id),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn drain_previous_deployment_marks_old_deployment_paused() {
        let Some(nats_client) =
            connect_nats_or_skip("drain_previous_deployment_marks_old_deployment_paused").await
        else {
            return;
        };

        let mut mock_app_repo = MockAppRepository::new();
        let mut mock_scheduler = crate::domain::MockScheduler::new();
        let old_dep_id = Uuid::new_v4();
        let app_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();

        mock_app_repo
            .expect_get_deployment()
            .with(eq(old_dep_id))
            .times(1)
            .returning(move |_| {
                Ok(Some(Deployment {
                    id: old_dep_id,
                    app_id,
                    job_id: Some("job-old".to_string()),
                    ..Default::default()
                }))
            });

        mock_app_repo
            .expect_get_app()
            .with(eq(app_id))
            .times(1)
            .returning(move |_| {
                Ok(Some(App {
                    id: app_id,
                    tenant_id: user_id,
                    name: "test-app".to_string(),
                    ..Default::default()
                }))
            });

        mock_scheduler
            .expect_pause_app()
            .with(eq("job-old".to_string()), eq("system".to_string()))
            .times(1)
            .returning(|_, _| Ok(true));

        mock_app_repo
            .expect_update_deployment()
            .with(
                eq(old_dep_id),
                function(|params: &crate::domain::UpdateDeploymentParams| {
                    params.status == Some("PAUSED".to_string())
                }),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        let state = AppState {
            ctx: crate::application::ApiContext::default(),
            user_repo: Arc::new(MockUserRepository::new()),
            database_repo: Arc::new(MockDatabaseRepository::new()),
            app_repo: Arc::new(mock_app_repo),
            github_repo: Arc::new(MockGithubRepository::default()),
            volume_repo: Arc::new(crate::domain::MockVolumeRepository::new()),
            scheduler: Arc::new(mock_scheduler),
            nats: crate::nats::TypedNatsClient::new(nats_client),
            router_addr: "http://localhost:8080".to_string(),
            frontend_url: "http://localhost:3000".to_string(),
            api_db: sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap(),
            jwt_secret: "secret".to_string(),
            master_key: "key".to_string(),
            deployment_events: tokio::sync::broadcast::channel(100).0,
            workspace_events: tokio::sync::broadcast::channel(100).0,
            mesh_status:
                tokio::sync::watch::channel(crate::application::vms::MeshStatus::default()).0,
            acme_email: "test@example.com".to_string(),
            acme_staging: true,
            acme_check_interval: 3600,
            github_app_id: None,
            github_private_key: None,
            github_app_slug: None,
            github_webhook_url_base: None,
            active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
        };

        DeploymentOrchestrator::drain_previous_deployment_after_promotion(
            &state,
            "test-app",
            Some(old_dep_id),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn mark_previous_deployment_draining_marks_old_deployment_draining() {
        let Some(nats_client) =
            connect_nats_or_skip("mark_previous_deployment_draining_marks_old_deployment_draining")
                .await
        else {
            return;
        };

        let mut mock_app_repo = MockAppRepository::new();
        let old_dep_id = Uuid::new_v4();
        let app_id = Uuid::new_v4();

        mock_app_repo
            .expect_get_deployment()
            .with(eq(old_dep_id))
            .times(1)
            .returning(move |_| {
                Ok(Some(Deployment {
                    id: old_dep_id,
                    job_id: Some("job-old".to_string()),
                    ..Default::default()
                }))
            });

        mock_app_repo
            .expect_update_deployment()
            .with(
                eq(old_dep_id),
                function(|params: &crate::domain::UpdateDeploymentParams| {
                    params.status == Some("DRAINING".to_string())
                }),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        let user_repo = Arc::new(MockUserRepository::new());
        let app_repo = Arc::new(mock_app_repo);
        let github_repo = Arc::new(MockGithubRepository::default());
        let volume_repo = Arc::new(crate::domain::MockVolumeRepository::new());
        let scheduler = Arc::new(crate::domain::MockScheduler::new());
        let nats = crate::nats::TypedNatsClient::new(nats_client);
        let db = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();

        let ctx = crate::application::ApiContext {
            user_repo: user_repo.clone(),
            database_repo: Arc::new(MockDatabaseRepository::new()),
            app_repo: app_repo.clone(),
            github_repo: github_repo.clone(),
            volume_repo: volume_repo.clone(),
            scheduler: scheduler.clone(),
            nats: nats.clone(),
            db: db.clone(),
            config: Arc::new(crate::config::ApiConfig::default()),
            jwt_secret: "secret".to_string(),
            master_key: "key".to_string(),
        };

        let state = AppState {
            ctx,
            user_repo,
            database_repo: Arc::new(MockDatabaseRepository::new()),
            app_repo,
            github_repo,
            volume_repo,
            scheduler,
            nats,
            router_addr: "http://localhost:8080".to_string(),
            frontend_url: "http://localhost:3000".to_string(),
            api_db: db,
            jwt_secret: "secret".to_string(),
            master_key: "key".to_string(),
            deployment_events: tokio::sync::broadcast::channel(100).0,
            workspace_events: tokio::sync::broadcast::channel(100).0,
            mesh_status:
                tokio::sync::watch::channel(crate::application::vms::MeshStatus::default()).0,
            acme_email: "test@example.com".to_string(),
            acme_staging: true,
            acme_check_interval: 3600,
            github_app_id: None,
            github_private_key: None,
            github_app_slug: None,
            github_webhook_url_base: None,
            active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
        };

        DeploymentOrchestrator::mark_previous_deployment_draining(
            &state,
            "test-app",
            app_id,
            Some(old_dep_id),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn rollback_failed_promotion_restores_previous_and_marks_new_failed() {
        let Some(nats_client) = connect_nats_or_skip(
            "rollback_failed_promotion_restores_previous_and_marks_new_failed",
        )
        .await
        else {
            return;
        };

        let mut mock_app_repo = MockAppRepository::new();
        let mut mock_scheduler = crate::domain::MockScheduler::new();
        let old_dep_id = Uuid::new_v4();
        let new_dep_id = Uuid::new_v4();
        let app_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();

        mock_app_repo
            .expect_update_deployment()
            .with(
                eq(old_dep_id),
                function(|params: &crate::domain::UpdateDeploymentParams| {
                    params.status == Some("RUNNING".to_string())
                }),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        mock_app_repo
            .expect_get_app()
            .with(eq(app_id))
            .times(1)
            .returning(move |_| {
                Ok(Some(App {
                    id: app_id,
                    tenant_id: user_id,
                    name: "test-app".to_string(),
                    ..Default::default()
                }))
            });

        mock_scheduler
            .expect_pause_app()
            .with(eq("job-new".to_string()), eq("system".to_string()))
            .times(1)
            .returning(|_, _| Ok(true));

        mock_app_repo
            .expect_update_deployment()
            .with(
                eq(new_dep_id),
                function(|params: &crate::domain::UpdateDeploymentParams| {
                    params.status == Some("FAILED".to_string())
                }),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        let state = AppState {
            ctx: crate::application::ApiContext::default(),
            user_repo: Arc::new(MockUserRepository::new()),
            database_repo: Arc::new(MockDatabaseRepository::new()),
            app_repo: Arc::new(mock_app_repo),
            github_repo: Arc::new(MockGithubRepository::default()),
            volume_repo: Arc::new(crate::domain::MockVolumeRepository::new()),
            scheduler: Arc::new(mock_scheduler),
            nats: crate::nats::TypedNatsClient::new(nats_client),
            router_addr: "http://localhost:8080".to_string(),
            frontend_url: "http://localhost:3000".to_string(),
            api_db: sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap(),
            jwt_secret: "secret".to_string(),
            master_key: "key".to_string(),
            deployment_events: tokio::sync::broadcast::channel(100).0,
            workspace_events: tokio::sync::broadcast::channel(100).0,
            mesh_status:
                tokio::sync::watch::channel(crate::application::vms::MeshStatus::default()).0,
            acme_email: "test@example.com".to_string(),
            acme_staging: true,
            acme_check_interval: 3600,
            github_app_id: None,
            github_private_key: None,
            github_app_slug: None,
            github_webhook_url_base: None,
            active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
        };

        DeploymentOrchestrator::rollback_failed_promotion(
            &state,
            "test-app",
            app_id,
            new_dep_id,
            "job-new",
            Some(old_dep_id),
        )
        .await
        .unwrap();
    }
}
