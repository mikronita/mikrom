use std::sync::Arc;

use mikrom_api::AppState;
use mikrom_api::infrastructure::db::PostgresAppRepository;
use mikrom_api::infrastructure::db::PostgresUserRepository;
use sqlx::PgPool;

use super::get_nats_client_or_skip;

pub async fn create_integration_app(pool: PgPool, jwt_secret: &str) -> Option<axum::Router> {
    let user_repo = PostgresUserRepository::new(pool.clone());
    let app_repo = PostgresAppRepository::new(pool.clone(), "test-key".to_string());
    let nats_client = get_nats_client_or_skip().await?;

    let mut mock_scheduler = mikrom_api::domain::MockScheduler::new();
    mock_scheduler
        .expect_delete_all_by_app()
        .returning(|_, _| Ok(true));
    mock_scheduler
        .expect_update_app_scaling_config()
        .returning(|_| Ok(true));
    mock_scheduler
        .expect_list_apps()
        .times(0..)
        .returning(|_| Ok(mikrom_proto::scheduler::ListAppsResponse::default()));

    let nats_clone = nats_client.clone();
    tokio::spawn(async move {
        use futures::StreamExt;
        use mikrom_proto::router::RouterConfigAck;
        use prost::Message;

        if let Ok(mut sub) = nats_clone
            .subscribe(mikrom_proto::subjects::ROUTER_CONFIG_UPDATED)
            .await
        {
            while let Some(msg) = sub.next().await {
                if let Some(reply) = msg.reply {
                    let ack = RouterConfigAck {
                        success: true,
                        message: String::new(),
                    };
                    let mut buf = Vec::new();
                    if ack.encode(&mut buf).is_ok() {
                        let _ = nats_clone.publish(reply, buf.into()).await;
                    }
                }
            }
        }
    });

    let mut mock_volume_repo = mikrom_api::domain::MockVolumeRepository::new();
    mock_volume_repo
        .expect_list_volumes_by_app()
        .returning(|_| Ok(vec![]));

    let state = AppState {
        ctx: mikrom_api::application::ApiContext::default(),
        user_repo: Arc::new(user_repo),
        tenant_repo: Arc::new(mikrom_api::domain::MockTenantRepository::new()),
        app_repo: Arc::new(app_repo),
        database_repo: Arc::new(mikrom_api::domain::MockDatabaseRepository::new()),
        volume_repo: Arc::new(mock_volume_repo),
        github_repo: Arc::new(mikrom_api::domain::github::MockGithubRepository::default()),
        scheduler: Arc::new(mock_scheduler),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        jwt_secret: jwt_secret.to_string(),
        master_key: "integration-master-key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        api_db: pool,
        acme_email: "admin@mikrom.spluca.org".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        mesh_status: tokio::sync::watch::channel(
            mikrom_api::application::vms::MeshStatus::default(),
        )
        .0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };
    Some(mikrom_api::create_app(state))
}
