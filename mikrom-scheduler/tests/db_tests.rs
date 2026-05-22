#[path = "common_utils.rs"]
mod common_utils;

#[cfg(test)]
mod tests {
    use super::common_utils;
    use mikrom_scheduler::domain::{AppConfig, AppRepository};
    use mikrom_scheduler::infrastructure::db::PgAppRepository;
    use sqlx::Row;

    #[tokio::test]
    async fn test_scheduler_migrations() {
        let db = common_utils::TestDb::new().await;
        let pool = db.pool().clone();

        // Verify tables exist
        let tables = sqlx::query(
            "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        let table_names: Vec<String> = tables
            .into_iter()
            .map(|row: sqlx::postgres::PgRow| row.get(0))
            .collect();

        assert!(table_names.contains(&"jobs".to_string()));
        assert!(table_names.contains(&"workers".to_string()));
    }

    #[tokio::test]
    async fn test_scheduler_app_config_persists_router_activity_and_hostname_lookup() {
        let db = common_utils::TestDb::new().await;
        let pool = db.pool().clone();
        let repo = PgAppRepository::new(pool.clone());

        let config = AppConfig {
            id: "app-1".to_string(),
            user_id: "user-1".to_string(),
            vpc_ipv6_prefix: "fd00:1234::".to_string(),
            hostname: "db-test.example.com".to_string(),
            desired_replicas: 2,
            min_replicas: 1,
            max_replicas: 4,
            autoscaling_enabled: true,
            cpu_threshold: 75.0,
            mem_threshold: 65.0,
            last_router_traffic_at: 12345,
            last_scaled_to_zero_at: 67890,
            restore_retry_after_at: 0,
        };

        repo.update_app_config(config.clone()).await.unwrap();

        let by_id = repo.get_app_config("app-1").await.unwrap().unwrap();
        assert_eq!(by_id.hostname, "db-test.example.com");
        assert_eq!(by_id.last_router_traffic_at, 12345);
        assert_eq!(by_id.last_scaled_to_zero_at, 67890);
        assert_eq!(by_id.restore_retry_after_at, 0);

        let by_hostname = repo
            .get_app_config_by_hostname("db-test.example.com")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(by_hostname.id, "app-1");
        assert_eq!(by_hostname.user_id, "user-1");
        assert_eq!(by_hostname.vpc_ipv6_prefix, "fd00:1234::");
        assert_eq!(by_hostname.desired_replicas, 2);
        assert!(by_hostname.autoscaling_enabled);
        assert_eq!(by_hostname.cpu_threshold, 75.0);
        assert_eq!(by_hostname.mem_threshold, 65.0);
    }
}
