#[cfg(test)]
mod tests {
    use sqlx::PgPool;
    use std::env;

    async fn get_test_pool() -> PgPool {
        let connection_string = env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
            "postgres://mikrom:mikrom_password@localhost:5433/mikrom_scheduler".to_string()
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
    #[ignore = "requires a running postgres at localhost:5433"]
    async fn test_scheduler_migrations() {
        let pool = get_test_pool().await;

        // Verify tables exist
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

        assert!(table_names.contains(&"workers".to_string()));
        assert!(table_names.contains(&"jobs".to_string()));
        assert!(table_names.contains(&"ip_allocations".to_string()));
    }
}
