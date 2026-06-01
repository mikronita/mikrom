#[path = "common_utils.rs"]
mod common_utils;

#[cfg(test)]
mod tests {
    use super::common_utils;
    use mikrom_scheduler::domain::{
        AppConfig, AppId, AppRepository, Job, JobId, JobRepository, TenantId, VmConfig,
    };
    use mikrom_scheduler::infrastructure::db::PgAppRepository;
    use mikrom_scheduler::infrastructure::db::PgJobRepository;
    use sqlx::Row;

    #[tokio::test]
    async fn test_scheduler_migrations() {
        let Ok(_db) = common_utils::TestDb::try_new().await else {
            eprintln!("Skipping db test: database unavailable");
            return;
        };
        let pool = _db.pool().clone();

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
        let Ok(_db) = common_utils::TestDb::try_new().await else {
            eprintln!("Skipping db test: database unavailable");
            return;
        };
        let pool = _db.pool().clone();
        let repo = PgAppRepository::new(pool.clone());

        let config = AppConfig {
            id: AppId::from("app-1".to_string()),
            tenant_id: TenantId::from("tenant-1".to_string()),
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

        assert_eq!(by_hostname.id, AppId::from("app-1".to_string()));
        assert_eq!(
            by_hostname.tenant_id,
            TenantId::from("tenant-1".to_string())
        );
        assert_eq!(by_hostname.vpc_ipv6_prefix, "fd00:1234::");
        assert_eq!(by_hostname.desired_replicas, 2);
        assert!(by_hostname.autoscaling_enabled);
        assert_eq!(by_hostname.cpu_threshold, 75.0);
        assert_eq!(by_hostname.mem_threshold, 65.0);
    }

    #[tokio::test]
    async fn test_scheduler_remove_app_and_jobs_by_app_cleans_app_row() {
        let Ok(_db) = common_utils::TestDb::try_new().await else {
            eprintln!("Skipping db test: database unavailable");
            return;
        };
        let pool = _db.pool().clone();
        let app_repo = PgAppRepository::new(pool.clone());
        let job_repo = PgJobRepository::new(pool.clone());

        let app = AppConfig {
            id: AppId::from("app-delete".to_string()),
            tenant_id: TenantId::from("tenant-delete".to_string()),
            vpc_ipv6_prefix: "fd00:abcd::".to_string(),
            hostname: "delete.example.com".to_string(),
            desired_replicas: 1,
            min_replicas: 0,
            max_replicas: 1,
            autoscaling_enabled: false,
            cpu_threshold: 80.0,
            mem_threshold: 80.0,
            last_router_traffic_at: 0,
            last_scaled_to_zero_at: 0,
            restore_retry_after_at: 0,
        };

        app_repo.update_app_config(app.clone()).await.unwrap();

        let job = Job::new(
            JobId::from("job-delete".to_string()),
            app.id.clone(),
            "delete-app".to_string(),
            "nginx:latest".to_string(),
            VmConfig::default(),
            app.tenant_id.clone(),
            None,
        );
        job_repo.add_job(job).await.unwrap();

        app_repo
            .remove_app_and_jobs_by_app(app.id.as_ref())
            .await
            .unwrap();

        assert!(
            app_repo
                .get_app_config(app.id.as_ref())
                .await
                .unwrap()
                .is_none()
        );
        assert!(job_repo.get_job("job-delete").await.unwrap().is_none());
    }
}
