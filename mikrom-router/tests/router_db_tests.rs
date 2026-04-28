#[cfg(test)]
mod tests {
    use hyper_util::client::legacy::connect::HttpConnector;
    use hyper_util::rt::TokioExecutor;
    use mikrom_router::resolver::{AppState, resolve_target};
    use moka::future::Cache;
    use sqlx::PgPool;
    use std::env;

    async fn get_test_pool() -> PgPool {
        let connection_string = env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
            "postgres://mikrom:mikrom_password@localhost:5432/mikrom_router_test".to_string()
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
    #[ignore = "requires a running postgres at localhost:5434"]
    async fn test_router_migrations_and_resolve() {
        let pool = get_test_pool().await;

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
