#[path = "common_utils.rs"]
mod common_utils;

#[cfg(test)]
mod tests {
    use super::common_utils;
    use hyper_util::client::legacy::connect::HttpConnector;
    use hyper_util::rt::TokioExecutor;
    use mikrom_router::resolver::{AppState, resolve_target};
    use moka::future::Cache;

    #[tokio::test]
    #[ignore = "requires a running postgres"]
    async fn test_router_migrations_and_resolve() {
        let test_db = common_utils::TestDb::new().await;
        let pool = test_db.pool().clone();

        // 1. Insert a test route
        sqlx::query("INSERT INTO routes (hostname, target_url) VALUES ($1, $2) ON CONFLICT (hostname) DO UPDATE SET target_url = EXCLUDED.target_url")
            .bind("test.mikrom.local")
            .bind("http://10.0.0.5:8080")
            .execute(&pool)
            .await
            .unwrap();

        let state = AppState {
            db: pool,
            cache: Cache::builder().build(),
            client: hyper_util::client::legacy::Client::builder(TokioExecutor::new())
                .build(HttpConnector::new()),
        };

        // 2. Resolve the route
        let target = resolve_target(&state, "test.mikrom.local").await.unwrap();
        assert_eq!(target, "http://10.0.0.5:8080");

        // 3. Check cache
        assert_eq!(
            state.cache.get("test.mikrom.local").await.unwrap(),
            "http://10.0.0.5:8080"
        );
    }
}
