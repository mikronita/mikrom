use sqlx::{Connection, Executor, PgConnection, PgPool, postgres::PgPoolOptions};
use std::env;
use std::ops::Deref;
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_TEST_DATABASE_URL: &str =
    "postgres://mikrom:mikrom_password@localhost:5432/mikrom_router_test";

static TEST_DB_REGISTRY: OnceLock<tokio::sync::Mutex<Option<Weak<TestDbInner>>>> = OnceLock::new();

pub struct TestDb {
    inner: Arc<TestDbInner>,
}

struct TestDbInner {
    pool: PgPool,
    db_name: String,
    server_url: String,
}

impl TestDb {
    pub async fn new() -> Self {
        Self::try_new()
            .await
            .expect("Failed to initialize test database")
    }

    pub async fn try_new() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();

        let registry = TEST_DB_REGISTRY.get_or_init(|| tokio::sync::Mutex::new(None));
        let mut guard = registry.lock().await;

        if let Some(db) = guard.as_ref().and_then(Weak::upgrade) {
            return Ok(Self { inner: db });
        }

        let inner = Arc::new(Self::create().await?);
        *guard = Some(Arc::downgrade(&inner));
        drop(guard);

        Ok(Self { inner })
    }

    async fn create() -> anyhow::Result<TestDbInner> {
        let test_url =
            env::var("TEST_DATABASE_URL").unwrap_or_else(|_| DEFAULT_TEST_DATABASE_URL.to_string());

        let (server_url, base_db_name) = split_url(&test_url);
        validate_test_database_name(&base_db_name)?;

        let db_name = format!(
            "{}_{}_{}",
            base_db_name,
            std::process::id(),
            unique_suffix()
        );
        let maintenance_url = format!("{server_url}/postgres");

        let mut conn = PgConnection::connect(&maintenance_url).await?;
        conn.execute(format!("CREATE DATABASE {db_name}").as_str())
            .await?;

        let pool_url = format!("{server_url}/{db_name}");
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&pool_url)
            .await?;

        sqlx::migrate!("./migrations").run(&pool).await?;

        Ok(TestDbInner {
            pool,
            db_name,
            server_url,
        })
    }

    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.inner.pool
    }
}

impl Deref for TestDb {
    type Target = PgPool;

    fn deref(&self) -> &Self::Target {
        &self.inner.pool
    }
}

impl Drop for TestDbInner {
    fn drop(&mut self) {
        let server_url = self.server_url.clone();
        let db_name = self.db_name.clone();
        let cleanup_db_name = db_name.clone();

        let handle = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build cleanup runtime");

            rt.block_on(async move {
                let maintenance_url = format!("{server_url}/postgres");
                let cleanup = async {
                    if let Ok(mut conn) = PgConnection::connect(&maintenance_url).await {
                        if let Err(e) = conn
                            .execute(format!(
                                "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{cleanup_db_name}' AND pid <> pg_backend_pid()"
                            ).as_str())
                            .await
                        {
                            eprintln!(
                                "warning: failed to terminate test database sessions for {cleanup_db_name}: {e}"
                            );
                        }

                        if let Err(e) = conn
                            .execute(format!("DROP DATABASE IF EXISTS \"{cleanup_db_name}\"").as_str())
                            .await
                        {
                            eprintln!(
                                "warning: failed to drop test database {cleanup_db_name}: {e}"
                            );
                        }
                    }
                };

                if tokio::time::timeout(Duration::from_secs(5), cleanup)
                    .await
                    .is_err()
                {
                    eprintln!(
                        "warning: test database cleanup timed out for {cleanup_db_name}"
                    );
                }
            });
        });

        if let Err(join_err) = handle.join() {
            eprintln!("warning: test database cleanup thread panicked for {db_name}: {join_err:?}");
        }
    }
}

fn validate_test_database_name(db_name: &str) -> anyhow::Result<()> {
    if db_name.is_empty() {
        anyhow::bail!("TEST_DATABASE_URL must include a database name ending in _test");
    }

    let is_safe_name = db_name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !is_safe_name {
        anyhow::bail!(
            "TEST_DATABASE_URL must use a simple test database name composed of letters, digits, and underscores"
        );
    }

    if !db_name.ends_with("_test") {
        anyhow::bail!(
            "TEST_DATABASE_URL must point to a test database name ending in _test, got {db_name}"
        );
    }

    Ok(())
}

fn split_url(url: &str) -> (String, String) {
    let last_slash = url.rfind('/').expect("Invalid database URL");
    let server_url = &url[..last_slash];
    let db_name = &url[last_slash + 1..];
    (server_url.to_string(), db_name.to_string())
}

fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before UNIX_EPOCH")
        .as_nanos();
    format!("{nanos:x}")
}
