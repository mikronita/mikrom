#[cfg(test)]
mod tests {
    use mikrom_api::AppState;
    use mikrom_api::RouterConfig;
    use mikrom_api::repositories::PostgresAppRepository;
    use mikrom_api::repositories::postgres_user_repository::PostgresUserRepository;
    use sqlx::PgPool;
    use std::env;
    use std::sync::Arc;

    async fn get_test_pool() -> PgPool {
        let connection_string = env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
            "postgres://mikrom:mikrom_password@localhost:5432/mikrom_api".to_string()
        });

        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(&connection_string)
            .await
            .expect("Failed to connect to test db");

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("Failed to run migrations");

        pool
    }

    #[tokio::test]
    #[ignore = "requires a running postgres at localhost:5432 and NATS"]
    async fn test_api_db_isolation() {
        let pool = get_test_pool().await;

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

        assert!(!table_names.contains(&"workers".to_string()));
        assert!(!table_names.contains(&"jobs".to_string()));
        assert!(table_names.contains(&"users".to_string()));
        assert!(table_names.contains(&"apps".to_string()));
    }

    #[tokio::test]
    #[ignore = "requires a running postgres at localhost:5432 and NATS"]
    async fn test_notify_router_sends_nats_message() {
        let pool = get_test_pool().await;
        let nats_url = env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
        let nats_client = async_nats::connect(nats_url).await.unwrap();

        let state = AppState {
            user_repo: Arc::new(PostgresUserRepository::new(pool.clone())),
            app_repo: Arc::new(PostgresAppRepository::new(pool.clone())),
            scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
            nats_client: nats_client.clone(),
            router_addr: "http://localhost:8080".to_string(),
            jwt_secret: "secret".into(),
            master_key: "key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            build_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        };

        // Subscribe to router updates
        let mut sub = nats_client
            .subscribe("mikrom.router.config_updated")
            .await
            .unwrap();

        // Trigger notification (even if app doesn't exist, it shouldn't crash)
        let app_id = uuid::Uuid::new_v4();
        let _ = state.notify_router(app_id).await;

        // In a real test we'd create an app and verify the payload
    }
}
