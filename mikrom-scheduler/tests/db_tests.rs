#[path = "common_utils.rs"]
mod common_utils;

#[cfg(test)]
mod tests {
    use super::common_utils;
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

        assert!(table_names.contains(&"workers".to_string()));
        assert!(table_names.contains(&"jobs".to_string()));
        assert!(table_names.contains(&"ip_allocations".to_string()));
    }
}
