use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures::StreamExt;
use mockall::predicate::*;
use std::sync::Arc;
use tokio::time::{Duration, timeout};
use tower::ServiceExt;
use uuid::Uuid;

use mikrom_api::AppState;
use mikrom_api::create_app;
use mikrom_api::domain::app::App;
use mikrom_api::domain::{
    MockAppRepository, MockScheduler, MockTenantRepository, MockUserRepository, Tenant,
};

const TENANT_SLUG: &str = "cleanup-tenant";

fn nats_integration_enabled() -> bool {
    if std::env::var("MIKROM_RUN_NATS_TESTS").is_err() {
        println!("Skipping NATS test: set MIKROM_RUN_NATS_TESTS=1 to run it");
        return false;
    }

    true
}

async fn connect_nats_or_skip() -> Option<async_nats::Client> {
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());

    match async_nats::connect(nats_url).await {
        Ok(client) => Some(client),
        Err(err) => {
            eprintln!(
                "skipping delete app cleanup test: unable to connect to NATS: {}",
                err
            );
            None
        },
    }
}

#[tokio::test]
#[ignore = "requires a NATS broker; run with MIKROM_RUN_NATS_TESTS=1 cargo test -p mikrom-api --test app_cleanup_integration_tests -- --ignored"]
async fn test_delete_app_triggers_bulk_cleanup() {
    if !nats_integration_enabled() {
        return;
    }

    let mock_user_repo = MockUserRepository::new();
    let mut mock_tenant_repo = MockTenantRepository::new();
    let mut mock_app_repo = MockAppRepository::new();
    let mut mock_scheduler = MockScheduler::new();

    let tenant_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let app_name = "cleanup-test-app";
    let jwt_secret = "test-secret";

    let token = mikrom_api::auth::jwt::create_token(
        &user_id.to_string(),
        "test@example.com",
        &mikrom_api::domain::user::UserRole::User,
        jwt_secret,
    )
    .unwrap();

    let app = App {
        id: app_id,
        name: app_name.to_string(),
        git_url: "git".to_string(),
        port: mikrom_api::domain::types::Port::new(8080).unwrap(),
        hostname: Some("test.example.com".to_string()),
        tenant_id,
        ..Default::default()
    };

    let Some(nats_client) = connect_nats_or_skip().await else {
        return;
    };

    mock_tenant_repo
        .expect_find_by_slug()
        .returning(move |slug| {
            Ok((slug == TENANT_SLUG).then_some(Tenant {
                id: tenant_id,
                tenant_id: TENANT_SLUG.to_string(),
                name: "Cleanup Tenant".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            }))
        });
    mock_tenant_repo
        .expect_is_member()
        .returning(move |tid, uid| Ok(tid == tenant_id && uid == user_id));

    let app_clone = app.clone();
    mock_app_repo
        .expect_get_app_by_name()
        .with(eq(app_name))
        .times(1)
        .returning(move |_| Ok(Some(app_clone.clone())));

    mock_app_repo
        .expect_delete_app()
        .with(eq(app_id))
        .times(1)
        .returning(|_| Ok(()));

    mock_scheduler
        .expect_delete_all_by_app()
        .with(eq(app_id.to_string()), eq(tenant_id.to_string()))
        .times(1)
        .returning(|_, _| Ok(true));

    let nats_responder = nats_client.clone();
    let mut route_sub = nats_responder
        .subscribe(mikrom_proto::subjects::ROUTER_CONFIG_UPDATED)
        .await
        .unwrap();
    tokio::spawn(async move {
        use mikrom_proto::router::{RouterConfigAck, RouterConfigUpdate};
        use prost::Message;

        if let Some(msg) = route_sub.next().await
            && let Ok(update) = RouterConfigUpdate::decode(&msg.payload[..])
        {
            assert_eq!(update.hostname, "test.example.com");
            assert!(update.target_urls.is_empty());

            let ack = RouterConfigAck {
                success: true,
                message: "deleted".to_string(),
            };
            let mut buf = Vec::new();
            ack.encode(&mut buf).unwrap();
            if let Some(reply) = msg.reply {
                let _ = nats_responder.publish(reply, buf.into()).await;
            }
        }
    });

    let mut mock_volume_repo = mikrom_api::domain::MockVolumeRepository::new();
    mock_volume_repo
        .expect_list_volumes_by_app()
        .returning(|_| Ok(vec![]));

    let (workspace_events, mut workspace_rx) = tokio::sync::broadcast::channel(100);

    let state = AppState {
        ctx: mikrom_api::application::ApiContext::default(),
        user_repo: Arc::new(mock_user_repo),
        tenant_repo: Arc::new(mock_tenant_repo),
        app_repo: Arc::new(mock_app_repo),
        database_repo: Arc::new(mikrom_api::domain::MockDatabaseRepository::new()),
        github_repo: Arc::new(mikrom_api::domain::github::MockGithubRepository::default()),
        volume_repo: Arc::new(mock_volume_repo),
        scheduler: Arc::new(mock_scheduler),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        api_db: sqlx::PgPool::connect_lazy("postgres://localhost/dummy").unwrap(),
        jwt_secret: jwt_secret.into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        workspace_events: workspace_events.clone(),
        mesh_status: tokio::sync::watch::channel(
            mikrom_api::application::vms::MeshStatus::default(),
        )
        .0,
        acme_email: "admin@mikrom.example.com".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        apps_domain: "apps.mikrom.example.com".to_string(),
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    let router = create_app(state);

    let response = router
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/apps/{}", app_name))
                .header("Authorization", format!("Bearer {}", token))
                .header("x-mikrom-tenant-id", TENANT_SLUG)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let event = timeout(Duration::from_millis(250), workspace_rx.recv())
        .await
        .expect("workspace event should be emitted")
        .unwrap();
    assert!(matches!(
        event.kind,
        mikrom_api::workspace::WorkspaceEventKind::AppDeleted
    ));
    assert_eq!(event.tenant_id, Some(tenant_id));
    assert_eq!(event.app_id, Some(app_id));
    assert_eq!(event.app_name.as_deref(), Some(app_name));
}
