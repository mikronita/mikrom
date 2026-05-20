use sqlx::{Connection, Executor, PgConnection, PgPool, postgres::PgPoolOptions};
use std::env;
use std::ops::Deref;

pub struct TestDb {
    pool: PgPool,
    db_name: String,
    server_url: String,
}

impl TestDb {
    pub async fn new() -> Self {
        dotenvy::dotenv().ok();
        let test_url = env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must be set for integration tests");

        let (server_url, base_db_name) = split_url(&test_url);
        // Use a unique name per test process to avoid conflicts during parallel execution
        let db_name = format!("{}_{}", base_db_name, uuid::Uuid::new_v4().simple());
        let maintenance_url = format!("{}/postgres", server_url);

        let mut conn = PgConnection::connect(&maintenance_url)
            .await
            .expect("Failed to connect to maintenance database");

        conn.execute(format!("CREATE DATABASE {}", db_name).as_str())
            .await
            .expect("Failed to create test database");

        let pool_url = format!("{}/{}", server_url, db_name);
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&pool_url)
            .await
            .expect("Failed to connect to test db");

        // Run migrations
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("Failed to run migrations");

        Self {
            pool,
            db_name,
            server_url,
        }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

impl Deref for TestDb {
    type Target = PgPool;

    fn deref(&self) -> &Self::Target {
        &self.pool
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        // Best-effort cleanup runs in the background so dropping the test helper stays non-blocking.
        let server_url = self.server_url.clone();
        let db_name = self.db_name.clone();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            rt.block_on(async {
                let maintenance_url = format!("{}/postgres", server_url);
                if let Ok(mut conn) = PgConnection::connect(&maintenance_url).await {
                    // Terminate other connections to be able to drop
                    let _ = conn.execute(format!(
                        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{}' AND pid <> pg_backend_pid()",
                        db_name
                    ).as_str()).await;

                    let _ = conn.execute(format!("DROP DATABASE IF EXISTS \"{}\"", db_name).as_str()).await;
                }
            });
        });
    }
}

fn split_url(url: &str) -> (String, String) {
    let last_slash = url.rfind('/').expect("Invalid database URL");
    let server_url = &url[..last_slash];
    let db_name = &url[last_slash + 1..];
    (server_url.to_string(), db_name.to_string())
}
