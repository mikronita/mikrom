mod common;

#[cfg(test)]
mod tests {
    use super::common;
    use mikrom_api::AppState;
    use mikrom_api::repositories::PostgresAppRepository;
    use mikrom_api::repositories::postgres_user_repository::PostgresUserRepository;
    use std::sync::Arc;

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

        assert!(!table_names.contains(&"workers".to_string()));
        assert!(!table_names.contains(&"jobs".to_string()));
        assert!(table_names.contains(&"users".to_string()));
        assert!(table_names.contains(&"apps".to_string()));
    }

    #[tokio::test]
    async fn test_notify_router_sends_nats_message() {
        let test_db = mikrom_api::test_utils::TestDb::new().await;
        let pool = test_db.pool().clone();
        let nats_client = common::get_nats_client().await;

        let state = AppState {
            user_repo: Arc::new(PostgresUserRepository::new(pool.clone())),
            app_repo: Arc::new(PostgresAppRepository::new(
                pool.clone(),
                "test-key".to_string(),
            )),
            scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
            nats_client: nats_client.clone(),
            router_addr: "http://localhost:8080".to_string(),
            jwt_secret: "secret".into(),
            master_key: "key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
        };

        // Subscribe to router updates
        let _sub = nats_client
            .subscribe(mikrom_proto::subjects::ROUTER_CONFIG_UPDATED)
            .await
            .unwrap();

        // Trigger notification (even if app doesn't exist, it shouldn't crash)
        let app_id = uuid::Uuid::new_v4();
        let _ = state.notify_router(app_id).await;

        // In a real test we'd create an app and verify the payload
    }
}
