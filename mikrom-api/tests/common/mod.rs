use async_nats::Client;
use sqlx::PgPool;
use std::env;

/// Returns a connected NATS client for testing.
/// Uses NATS_URL environment variable or defaults to localhost.
pub async fn get_nats_client() -> Client {
    let nats_url = env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
    async_nats::connect(nats_url)
        .await
        .expect("Failed to connect to NATS for testing")
}

/// Returns a connected Postgres pool for testing and runs migrations.
/// Uses TEST_DATABASE_URL environment variable or defaults to mikrom_api_test.
pub async fn get_test_pool() -> PgPool {
    let connection_string = env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://mikrom:mikrom_password@localhost:5432/mikrom_api_test".to_string()
    });

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&connection_string)
        .await
        .expect("Failed to connect to test db");

    // Run migrations to ensure the database schema is up to date
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    pool
}
