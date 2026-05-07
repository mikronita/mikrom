use futures::StreamExt;
use mikrom_api::AppState;
use mikrom_api::models::app::{App, Deployment};
use mikrom_api::repositories::app_repository::UpdateDeploymentParams;
use mikrom_api::repositories::{MockAppRepository, MockGithubRepository, MockUserRepository};
use mikrom_api::scheduler::MockScheduler;
use mikrom_proto::scheduler::{CheckHealthRequest, CheckHealthResponse, DeployResponse};
use mockall::predicate::*;
use prost::Message;
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
        ip_address: "10.0.0.2".to_string(),
        host_id: "host-1".to_string(),
        vm_id: "vm-new".to_string(),
        message: "Scheduled".to_string(),
    };

    // Mocks for run_zero_downtime_flow
    let app_clone = app.clone();
    mock_app_repo
        .expect_get_app()
        .with(eq(app_id))
        .returning(move |_| Ok(Some(app_clone.clone())));

    mock_app_repo
        .expect_set_active_deployment()
        .with(eq(app_id), eq(new_dep_id))
        .times(1)
        .returning(|_, _| Ok(()));

    // Old deployment cleanup (after 10s sleep)
    mock_app_repo
        .expect_get_deployment()
        .with(eq(old_dep_id))
        .returning(move |_| {
            Ok(Some(Deployment {
                id: old_dep_id,
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
                params.status == Some("STOPPED".to_string())
            }),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();

    // Subscribe to health check requests to respond
    let mut health_sub = nats_client
        .subscribe("mikrom.scheduler.check_health")
        .await
        .unwrap();

    let state = AppState {
        user_repo: Arc::new(MockUserRepository::new()),
        app_repo: Arc::new(mock_app_repo),
        github_repo: Arc::new(MockGithubRepository::default()),
        scheduler: Arc::new(mock_scheduler),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client.clone()),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        api_db: sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap(),
        jwt_secret: "secret".to_string(),
        master_key: "key".to_string(),
        deployment_events: tokio::sync::broadcast::channel(100).0,
        acme_email: "test@example.com".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
    };

    // Start zero-downtime flow
    mikrom_api::deploy::service::DeploymentService::run_zero_downtime_flow(
        state.clone(),
        app,
        new_deployment,
        inner,
        user_id.to_string(),
    );

    // 1. Handle health check request
    let msg = tokio::time::timeout(Duration::from_secs(5), health_sub.next())
        .await
        .expect("Timeout waiting for health check request")
        .expect("No health check request");

    let _req = CheckHealthRequest::decode(&msg.payload[..]).unwrap();
    let resp = CheckHealthResponse {
        is_healthy: true,
        message: "Healthy".to_string(),
    };
    let mut buf = Vec::new();
    resp.encode(&mut buf).unwrap();
    nats_client
        .publish(msg.reply.unwrap(), buf.into())
        .await
        .unwrap();

    // Now we wait for the flow to complete.
    // We'll wait up to 15s.
    tokio::time::sleep(Duration::from_secs(12)).await;
}
