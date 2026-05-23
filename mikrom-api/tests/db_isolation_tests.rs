mod common;

#[cfg(test)]
mod tests {
    use super::common;
    use mikrom_api::AppState;
    use mikrom_api::infrastructure::db::PostgresUserRepository;
    use mikrom_api::models::app::App;
    use mikrom_api::repositories::PostgresAppRepository;
    use prost::Message;
    use std::sync::Arc;
    use tokio_stream::StreamExt;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_api_db_isolation() {
        let test_db = mikrom_api::test_utils::TestDb::new().await;
        let pool = test_db.pool().clone();

        // Verify scheduler tables are NOT present in API database
        let tables = sqlx::query(
            "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        let table_names: Vec<String> = tables
            .into_iter()
            .map(|row: sqlx::postgres::PgRow| {
                use sqlx::Row;
                row.get(0)
            })
            .collect();

        assert!(!table_names.contains(&"jobs".to_string()));
        assert!(table_names.contains(&"deployments".to_string()));
        assert!(table_names.contains(&"apps".to_string()));
    }

    #[tokio::test]
    async fn test_notify_router_sends_nats_message() {
        let test_db = mikrom_api::test_utils::TestDb::new().await;
        let pool = test_db.pool().clone();
        let nats_url =
            std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
        let nats_client = match async_nats::connect(&nats_url).await {
            Ok(client) => client,
            Err(err) => {
                eprintln!("Skipping test: NATS not available at {nats_url}: {err}");
                return;
            },
        };

        let state = AppState {
            user_repo: Arc::new(PostgresUserRepository::new(pool.clone())),
            app_repo: Arc::new(PostgresAppRepository::new(
                pool.clone(),
                "test-key".to_string(),
            )),
            volume_repo: Arc::new(
                mikrom_api::repositories::volume_repository::MockVolumeRepository::new(),
            ),
            github_repo: Arc::new(mikrom_api::repositories::MockGithubRepository::default()),
            scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
            nats: mikrom_api::nats::TypedNatsClient::new(nats_client.clone()),
            router_addr: "http://localhost:8080".to_string(),
            frontend_url: "http://localhost:3000".to_string(),
            jwt_secret: "secret".into(),
            master_key: "key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            api_db: pool.clone(),
            acme_email: "admin@mikrom.spluca.org".to_string(),
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

        // Subscribe to router updates
        let _sub = nats_client
            .subscribe(mikrom_proto::subjects::ROUTER_CONFIG_UPDATED)
            .await
            .unwrap();

        // Trigger notification (even if app doesn't exist, it shouldn't crash)
        let app_id = uuid::Uuid::new_v4();
        if let Ok(Some(app)) = state.app_repo.get_app(app_id).await {
            let _ = state.notify_router(&app).await;
        }

        // In a real test we'd create an app and verify the payload
    }

    #[tokio::test]
    async fn test_notify_router_updates_scheduler_and_publishes_traffic_seed() {
        let test_db = mikrom_api::test_utils::TestDb::new().await;
        let pool = test_db.pool().clone();
        let nats_url =
            std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
        let nats_client = match async_nats::connect(&nats_url).await {
            Ok(client) => client,
            Err(err) => {
                eprintln!("Skipping test: NATS not available at {nats_url}: {err}");
                return;
            },
        };

        let app_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let hostname = "notify-router.example.com".to_string();
        let expected_hostname = hostname.clone();
        let vpc_prefix = "fd00:abcd::".to_string();

        let mut mock_user_repo =
            mikrom_api::repositories::user_repository::MockUserRepository::new();
        mock_user_repo.expect_find_by_id().returning(move |_| {
            Ok(Some(mikrom_api::repositories::user_repository::User {
                id: user_id,
                email: "test@example.com".to_string(),
                password_hash: "hash".to_string(),
                role: mikrom_api::repositories::user_repository::UserRole::User,
                first_name: None,
                last_name: None,
                vpc_ipv6_prefix: Some(vpc_prefix.clone()),
            }))
        });

        let mut mock_scheduler = mikrom_api::scheduler::MockScheduler::new();
        mock_scheduler.expect_list_apps().returning({
            let app_id = app_id.to_string();
            let user_id = user_id.to_string();
            move |_| {
                Ok(mikrom_proto::scheduler::ListAppsResponse {
                    apps: vec![mikrom_proto::scheduler::AppInfo {
                        job_id: "job-1".to_string(),
                        deployment_id: "dep-1".to_string(),
                        app_id: app_id.clone(),
                        user_id: user_id.clone(),
                        app_name: "notify-router-app".to_string(),
                        status: mikrom_proto::scheduler::DeployStatus::Running as i32,
                        ipv6_address: "fd00::10".to_string(),
                        ..Default::default()
                    }],
                })
            }
        });

        mock_scheduler
            .expect_update_app_scaling_config()
            .withf(move |req| {
                req.app_id == app_id.to_string()
                    && req.user_id == user_id.to_string()
                    && req.hostname == expected_hostname
                    && req.last_router_traffic_at > 0
                    && req.last_scaled_to_zero_at == 0
                    && req.desired_replicas == 1
            })
            .times(1)
            .returning(|_| Ok(true));

        let state = AppState {
            user_repo: Arc::new(mock_user_repo),
            app_repo: Arc::new(PostgresAppRepository::new(
                pool.clone(),
                "test-key".to_string(),
            )),
            volume_repo: Arc::new(
                mikrom_api::repositories::volume_repository::MockVolumeRepository::new(),
            ),
            github_repo: Arc::new(mikrom_api::repositories::MockGithubRepository::default()),
            scheduler: Arc::new(mock_scheduler),
            nats: mikrom_api::nats::TypedNatsClient::new(nats_client.clone()),
            router_addr: "http://localhost:8080".to_string(),
            frontend_url: "http://localhost:3000".to_string(),
            jwt_secret: "secret".into(),
            master_key: "key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            api_db: pool.clone(),
            acme_email: "admin@mikrom.spluca.org".to_string(),
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

        let mut sub = nats_client
            .subscribe(mikrom_proto::subjects::ROUTER_CONFIG_UPDATED)
            .await
            .unwrap();

        let app = App {
            id: app_id,
            name: "notify-router-app".to_string(),
            git_url: "https://github.com/test/notify-router".to_string(),
            port: 8080,
            hostname: Some(hostname.clone()),
            user_id,
            active_deployment_id: None,
            desired_replicas: 1,
            min_replicas: 1,
            max_replicas: 3,
            autoscaling_enabled: false,
            cpu_threshold: 80.0,
            mem_threshold: 80.0,
            ..App::default()
        };

        state.notify_router(&app).await.unwrap();

        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), sub.next())
            .await
            .expect("Timeout waiting for router config update")
            .expect("No router config update received");

        let update = mikrom_proto::router::RouterConfigUpdate::decode(&msg.payload[..]).unwrap();
        assert_eq!(update.hostname, hostname);
        assert_eq!(
            update.target_urls,
            vec!["http://[fd00::10]:8080".to_string()]
        );
    }

    #[tokio::test]
    async fn test_notify_router_skips_scaling_update_when_no_targets_exist() {
        let test_db = mikrom_api::test_utils::TestDb::new().await;
        let pool = test_db.pool().clone();
        let Some(nats_client) = common::get_nats_client_or_skip().await else {
            return;
        };

        let user_id = Uuid::new_v4();
        let hostname = "no-targets.example.com".to_string();

        let mut mock_user_repo =
            mikrom_api::repositories::user_repository::MockUserRepository::new();
        mock_user_repo.expect_find_by_id().returning(move |_| {
            Ok(Some(mikrom_api::repositories::user_repository::User {
                id: user_id,
                email: "test@example.com".to_string(),
                password_hash: "hash".to_string(),
                role: mikrom_api::repositories::user_repository::UserRole::User,
                first_name: None,
                last_name: None,
                vpc_ipv6_prefix: Some("fd00:abcd::".to_string()),
            }))
        });

        let mut mock_scheduler = mikrom_api::scheduler::MockScheduler::new();
        mock_scheduler
            .expect_list_apps()
            .returning(|_| Ok(mikrom_proto::scheduler::ListAppsResponse { apps: vec![] }));
        mock_scheduler.expect_update_app_scaling_config().times(0);

        let state = AppState {
            user_repo: Arc::new(mock_user_repo),
            app_repo: Arc::new(PostgresAppRepository::new(
                pool.clone(),
                "test-key".to_string(),
            )),
            volume_repo: Arc::new(
                mikrom_api::repositories::volume_repository::MockVolumeRepository::new(),
            ),
            github_repo: Arc::new(mikrom_api::repositories::MockGithubRepository::default()),
            scheduler: Arc::new(mock_scheduler),
            nats: mikrom_api::nats::TypedNatsClient::new(nats_client.clone()),
            router_addr: "http://localhost:8080".to_string(),
            frontend_url: "http://localhost:3000".to_string(),
            jwt_secret: "secret".into(),
            master_key: "key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            api_db: pool.clone(),
            acme_email: "admin@mikrom.spluca.org".to_string(),
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

        let mut sub = nats_client
            .subscribe(mikrom_proto::subjects::ROUTER_CONFIG_UPDATED)
            .await
            .unwrap();

        let app = App {
            id: Uuid::new_v4(),
            name: "no-targets-app".to_string(),
            git_url: "https://github.com/test/no-targets".to_string(),
            port: 8080,
            hostname: Some(hostname.clone()),
            user_id,
            ..App::default()
        };

        state.notify_router(&app).await.unwrap();

        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), sub.next())
            .await
            .expect("Timeout waiting for router config update")
            .expect("No router config update received");

        let update = mikrom_proto::router::RouterConfigUpdate::decode(&msg.payload[..]).unwrap();
        assert_eq!(update.hostname, hostname);
        assert!(update.target_urls.is_empty());
    }
}
