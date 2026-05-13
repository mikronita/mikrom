use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures::StreamExt;
use mockall::predicate::*;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

use mikrom_api::AppState;
use mikrom_api::create_app;
use mikrom_api::models::app::App;
use mikrom_api::repositories::{MockAppRepository, MockUserRepository};
use mikrom_api::scheduler::MockScheduler;

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
async fn test_delete_app_triggers_bulk_cleanup() {
    let mock_user_repo = MockUserRepository::new();
    let mut mock_app_repo = MockAppRepository::new();
    let mut mock_scheduler = MockScheduler::new();

    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let app_name = "cleanup-test-app";
    let jwt_secret = "test-secret";

    let token = mikrom_api::auth::jwt::create_token(
        &user_id.to_string(),
        "test@example.com",
        &mikrom_api::repositories::user_repository::UserRole::User,
        jwt_secret,
    )
    .unwrap();

    let app = App {
        id: app_id,
        name: app_name.to_string(),
        git_url: "git".to_string(),
        port: 8080,
        hostname: Some("test.example.com".to_string()),
        user_id,
        ..Default::default()
    };

    let Some(nats_client) = connect_nats_or_skip().await else {
        return;
    };

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

    // CRITICAL: Verify that delete_all_by_app is called on the scheduler
    mock_scheduler
        .expect_delete_all_by_app()
        .with(eq(app_id.to_string()), eq(user_id.to_string()))
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
            assert!(update.target_url.is_none());

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

    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        github_repo: Arc::new(mikrom_api::repositories::MockGithubRepository::default()),
        scheduler: Arc::new(mock_scheduler),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        jwt_secret: jwt_secret.into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        api_db: sqlx::PgPool::connect_lazy("postgres://localhost/dummy").unwrap(),
        acme_email: "admin@mikrom.spluca.org".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
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
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}
