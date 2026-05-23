mod common;
use futures::StreamExt;
use mikrom_api::domain::MockAppRepository;
use mikrom_api::domain::UpdateDeploymentParams;
use mikrom_api::domain::app::{App, Deployment};
use mikrom_api::scheduler::MockScheduler;
use mikrom_proto::scheduler::{CheckHealthResponse, DeployResponse};
use mockall::predicate::eq;
use std::sync::Arc;
use tokio::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn test_zero_downtime_flow_success() {
    let mut mock_app_repo = MockAppRepository::new();
    let mut mock_scheduler = MockScheduler::new();

    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let old_dep_id = Uuid::new_v4();
    let new_dep_id = Uuid::new_v4();

    let app = App {
        id: app_id,
        name: "test-app".to_string(),
        user_id,
        active_deployment_id: Some(old_dep_id),
        ..Default::default()
    };

    let new_deployment = Deployment {
        id: new_dep_id,
        app_id,
        user_id,
        status: "SCHEDULED".to_string(),
        ..Default::default()
    };

    let inner = DeployResponse {
        job_id: "job-new".to_string(),
        status: 2, // Scheduled
        host_id: "host-1".to_string(),
        vm_id: "vm-new".to_string(),
        message: "Scheduled".to_string(),
    };

    // Mocks for run_zero_downtime_flow
    let call_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let app_clone = app.clone();
    mock_app_repo
        .expect_get_app()
        .with(eq(app_id))
        .returning(move |_| {
            let count = call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut a = app_clone.clone();
            if count > 0 {
                a.active_deployment_id = Some(new_dep_id);
            }
            Ok(Some(a))
        });

    mock_app_repo
        .expect_set_active_deployment()
        .with(eq(app_id), eq(new_dep_id))
        .times(1)
        .returning(|_, _| Ok(()));

    // Old deployment cleanup now happens immediately after promotion.
    mock_app_repo
        .expect_get_deployment()
        .with(eq(old_dep_id))
        .times(2)
        .returning(move |_| {
            Ok(Some(Deployment {
                id: old_dep_id,
                app_id,
                job_id: Some("job-old".to_string()),
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
            mockall::predicate::function(|params: &UpdateDeploymentParams| {
                params.status == Some("DRAINING".to_string())
            }),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    mock_app_repo
        .expect_update_deployment()
        .with(
            eq(old_dep_id),
            mockall::predicate::function(|params: &UpdateDeploymentParams| {
                params.status == Some("PAUSED".to_string())
            }),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    mock_app_repo.expect_get_app().with(eq(app_id)).returning({
        let app = app.clone();
        move |_| Ok(Some(app.clone()))
    });
    let Some(nats_client) = common::get_nats_client_or_skip().await else {
        return;
    };

    let job_id = format!("job-zero-downtime-{}", Uuid::new_v4());
    let job_id_clone = job_id.clone();

    // Subscribe to health check requests to respond (only for our job_id)
    let nats_clone = nats_client.clone();
    tokio::spawn(async move {
        use mikrom_proto::scheduler::CheckHealthRequest;
        use prost::Message;

        let mut health_sub = nats_clone
            .subscribe("mikrom.scheduler.check_health")
            .await
            .unwrap();

        while let Some(msg) = health_sub.next().await {
            if let Ok(req) = CheckHealthRequest::decode(&msg.payload[..])
                && req.job_id != job_id_clone
            {
                continue;
            }

            let resp = CheckHealthResponse {
                is_healthy: true,
                message: "Healthy".to_string(),
            };
            let mut buf = Vec::new();
            resp.encode(&mut buf).unwrap();
            let _ = nats_clone.publish(msg.reply.unwrap(), buf.into()).await;
        }
    });

    let mut state = mikrom_api::test_utils::create_test_app_state(
        sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap(),
    );
    state.app_repo = Arc::new(mock_app_repo);
    state.scheduler = Arc::new(mock_scheduler);
    state.ctx.app_repo = state.app_repo.clone();
    state.ctx.scheduler = state.scheduler.clone();
    state.nats = mikrom_api::nats::TypedNatsClient::new(nats_client.clone());
    state.ctx.nats = state.nats.clone();

    let guard = state.try_start_flow(app_id.into()).unwrap();

    // Start zero-downtime flow
    mikrom_api::application::deployment::service::DeploymentService::run_zero_downtime_flow(
        state.clone(),
        app,
        new_deployment,
        DeployResponse {
            job_id: job_id.clone(),
            ..inner
        },
        user_id.to_string(),
        true,
        guard,
    );

    // Give the background task a moment to finish the immediate pause.
    tokio::time::sleep(Duration::from_secs(2)).await;
}

#[tokio::test]
async fn test_activate_deployment_no_job_id() {
    use axum::extract::{Path, State};
    use axum::http::StatusCode;
    use mikrom_api::auth::AuthUser;
    use mikrom_api::deploy::handlers::__activate_deployment_handler_impl as activate_deployment_handler;

    let mut mock_app_repo = MockAppRepository::new();
    let mock_scheduler = MockScheduler::new();

    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let deployment_id = Uuid::new_v4();

    let app = App {
        id: app_id,
        name: "test-app".to_string(),
        user_id,
        ..Default::default()
    };

    let deployment = Deployment {
        id: deployment_id,
        app_id,
        user_id,
        job_id: None, // NO JOB ID
        ..Default::default()
    };

    // Mocks
    mock_app_repo
        .expect_get_app_by_name()
        .returning(move |_| Ok(Some(app.clone())));

    mock_app_repo
        .expect_get_deployment()
        .with(eq(deployment_id))
        .returning(move |_| Ok(Some(deployment.clone())));

    mock_app_repo
        .expect_set_active_deployment()
        .with(eq(app_id), eq(deployment_id))
        .times(1)
        .returning(|_, _| Ok(()));
    let Some(nats_client) = common::get_nats_client_or_skip().await else {
        return;
    };

    let mut state = mikrom_api::test_utils::create_test_app_state(
        sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap(),
    );
    state.app_repo = Arc::new(mock_app_repo);
    state.scheduler = Arc::new(mock_scheduler);
    state.ctx.app_repo = state.app_repo.clone();
    state.ctx.scheduler = state.scheduler.clone();
    state.nats = mikrom_api::nats::TypedNatsClient::new(nats_client.clone());
    state.ctx.nats = state.nats.clone();

    let auth = AuthUser {
        user_id: user_id.to_string(),
        email: "test@example.com".to_string(),
        role: mikrom_api::domain::user::UserRole::User,
    };

    let result = activate_deployment_handler(
        auth,
        State(state),
        Path(("test-app".to_string(), deployment_id)),
    )
    .await;

    let status = result.unwrap();
    assert_eq!(status, StatusCode::OK); // Record-only activation should return 200 OK immediately
}
