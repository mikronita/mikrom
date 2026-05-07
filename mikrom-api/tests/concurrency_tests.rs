use futures::StreamExt;
use mikrom_api::AppState;
use mikrom_api::models::app::{App, Deployment};
use mikrom_api::repositories::{MockAppRepository, MockGithubRepository, MockUserRepository};
use mikrom_api::scheduler::MockScheduler;
use mikrom_proto::scheduler::{CheckHealthResponse, DeployResponse};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn test_concurrent_flows_prevented() {
    let mut mock_app_repo = MockAppRepository::new();
    let mock_scheduler = MockScheduler::new();

    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let dep_id = Uuid::new_v4();

    let app = App {
        id: app_id,
        name: "test-app".to_string(),
        user_id,
        ..Default::default()
    };

    let deployment = Deployment {
        id: dep_id,
        app_id,
        user_id,
        status: "RUNNING".to_string(),
        job_id: Some("job-1".to_string()),
        ..Default::default()
    };

    // We expect health check to be called.
    // We'll count how many times the flow starts or proceeds.
    // Actually, we want to verify that the second call returns early or is ignored.

    let app_clone = app.clone();
    mock_app_repo
        .expect_get_app()
        .returning(move |_| Ok(Some(app_clone.clone())));

    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();
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
        active_deployment_flows: Arc::new(dashmap::DashSet::new()),
    };

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = call_count.clone();

    // Respond to health checks and count them
    tokio::spawn(async move {
        while let Some(msg) = health_sub.next().await {
            cc.fetch_add(1, Ordering::SeqCst);
            let resp = CheckHealthResponse {
                is_healthy: false,
                message: "Unhealthy".to_string(),
            };
            let mut buf = Vec::new();
            use prost::Message;
            resp.encode(&mut buf).unwrap();
            let _ = nats_client.publish(msg.reply.unwrap(), buf.into()).await;
        }
    });

    // Start flow 1
    let guard1 = state.try_start_flow(app.id).expect("Flow 1 should start");
    mikrom_api::deploy::service::DeploymentService::run_zero_downtime_flow(
        state.clone(),
        app.clone(),
        deployment.clone(),
        DeployResponse {
            job_id: "job-1".to_string(),
            ..Default::default()
        },
        user_id.to_string(),
        false,
        guard1,
    );

    // Start flow 2 (concurrently) - it should fail to acquire guard
    let guard2_opt = state.try_start_flow(app.id);
    assert!(guard2_opt.is_none(), "Flow 2 should be prevented");

    // Wait a bit
    tokio::time::sleep(Duration::from_millis(500)).await;

    // If both flows are running, we'd expect at least 2 health check requests (one from each)
    // If concurrent flows are prevented, we expect only 1 (or requests only from one flow)
    // Note: each flow waits 2s between attempts.

    assert_eq!(
        call_count.load(Ordering::SeqCst),
        1,
        "Should only have one active flow polling health"
    );
}
