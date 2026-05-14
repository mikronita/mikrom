use futures::StreamExt;
use mikrom_api::AppState;
use mikrom_api::repositories::app_repository::{
    AppRepository, NewDeployment, UpdateDeploymentParams,
};
use mikrom_api::repositories::postgres_app_repository::PostgresAppRepository;
use mikrom_api::test_utils::TestDb;
use std::sync::Arc;
use tokio::time::{Duration, timeout};

#[tokio::test]
async fn test_route_reconciliation_on_startup() {
    let db = TestDb::new().await;
    let pool = db.pool().clone();
    let app_repo = Arc::new(PostgresAppRepository::new(pool.clone(), "test-key".into()));

    // 1. Setup NATS client using the project's test environment variable
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());

    // We attempt to connect. If NATS is not running, we skip the test instead of failing,
    // as it depends on external infrastructure in CI/Local Dev.
    let nats_client = match async_nats::connect(&nats_url).await {
        Ok(client) => client,
        Err(_) => {
            eprintln!("Skipping test: NATS not available at {}", nats_url);
            return;
        },
    };

    let state = AppState {
        app_repo: app_repo.clone(),
        user_repo: Arc::new(
            mikrom_api::repositories::postgres_user_repository::PostgresUserRepository::new(
                pool.clone(),
            ),
        ),
        volume_repo: Arc::new(
            mikrom_api::repositories::volume_repository::MockVolumeRepository::new(),
        ),
        github_repo: Arc::new(mikrom_api::repositories::MockGithubRepository::default()),
        scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client.clone()),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        jwt_secret: "test-secret".to_string(),
        master_key: "test-key".into(),
        api_db: pool.clone(),
        deployment_events: tokio::sync::broadcast::channel(100).0,
        acme_email: "test@example.com".into(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        mesh_status: tokio::sync::watch::channel(mikrom_api::vms::MeshStatus::default()).0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    // 2. Create test data: an app with an active deployment
    let user_id = uuid::Uuid::new_v4();
    // We need to insert a user first because of FK constraints
    sqlx::query("INSERT INTO users (id, email, password_hash, role) VALUES ($1, $2, $3, $4)")
        .bind(user_id)
        .bind(format!("test_{}@reconcile.com", uuid::Uuid::new_v4()))
        .bind("hash")
        .bind("user")
        .execute(&pool)
        .await
        .unwrap();

    let app = app_repo
        .create_app(mikrom_api::repositories::app_repository::CreateAppParams {
            name: "reconcile-app".to_string(),
            git_url: "https://github.com/test/reconcile".to_string(),
            port: 8080,
            hostname: Some("reconcile.mikrom.local".into()),
            user_id,
            ..Default::default()
        })
        .await
        .unwrap();

    let dep = app_repo
        .create_deployment(NewDeployment {
            app_id: app.id,
            user_id: user_id.to_string(),
            vcpus: 1,
            memory_mib: 128,
            disk_mib: 512,
            port: 8080,
            env_vars: std::collections::HashMap::new(),
            trigger_source: "test".into(),
            git_commit_hash: None,
            git_commit_message: None,
            git_branch: None,
        })
        .await
        .unwrap();

    // Mark deployment as running with an IP
    app_repo
        .update_deployment(
            dep.id,
            UpdateDeploymentParams {
                status: Some("RUNNING".into()),
                ipv6_address: Some("fd00::1".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    // Set as active deployment
    app_repo
        .set_active_deployment(app.id, dep.id)
        .await
        .unwrap();

    // 3. Subscribe to the router config update subject
    let mut sub = nats_client
        .subscribe(mikrom_proto::subjects::ROUTER_CONFIG_UPDATED)
        .await
        .unwrap();

    // 4. Run reconciliation
    state
        .reconcile_routes()
        .await
        .expect("Reconciliation failed");

    // 5. Verify a message was published
    let msg = timeout(Duration::from_secs(2), sub.next())
        .await
        .expect("Timeout waiting for reconciliation message")
        .expect("No message received");

    use mikrom_proto::router::RouterConfigUpdate;
    use prost::Message;
    let update = RouterConfigUpdate::decode(&msg.payload[..]).unwrap();

    assert_eq!(update.hostname, "reconcile.mikrom.local");
    assert_eq!(update.target_url, Some("http://[fd00::1]:8080".into()));
}
