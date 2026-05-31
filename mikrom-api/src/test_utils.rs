use crate::AppState;
use crate::application::ApiContext;
use crate::application::vms::MeshStatus;
use crate::domain::{
    MockAppRepository, MockDatabaseRepository, MockGithubRepository, MockScheduler,
    MockUserRepository, MockVolumeRepository,
};
use crate::infrastructure::nats::{MockNatsClient, TypedNatsClient};
use sqlx::{Connection, Executor, PgConnection, PgPool, postgres::PgPoolOptions};
use std::env;
use std::ops::Deref;
use std::sync::Arc;

pub struct TestDb {
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
        let test_url = env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
            "postgres://mikrom:mikrom_password@localhost:5432/mikrom_test".to_string()
        });

        let (server_url, base_db_name) = split_url(&test_url);
        // Use a unique name per test process to avoid conflicts during parallel execution
        let db_name = format!("{}_{}", base_db_name, uuid::Uuid::new_v4().simple());
        let maintenance_url = format!("{}/postgres", server_url);

        let mut conn = PgConnection::connect(&maintenance_url).await?;

        conn.execute(format!("CREATE DATABASE {}", db_name).as_str())
            .await?;

        let pool_url = format!("{}/{}", server_url, db_name);
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&pool_url)
            .await?;

        // Run migrations
        sqlx::migrate!("./migrations").run(&pool).await?;

        Ok(Self {
            pool,
            db_name,
            server_url,
        })
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

pub fn create_test_app_state(db: PgPool) -> AppState {
    let (deployment_events, _) = tokio::sync::broadcast::channel(100);
    let (workspace_events, _) = tokio::sync::broadcast::channel(100);
    let (mesh_status, _) = tokio::sync::watch::channel(MeshStatus::default());

    let user_repo = Arc::new(MockUserRepository::new());
    let app_repo = Arc::new(MockAppRepository::new());
    let database_repo = Arc::new(MockDatabaseRepository::new());
    let github_repo = Arc::new(MockGithubRepository::new());
    let volume_repo = Arc::new(MockVolumeRepository::new());
    let scheduler = Arc::new(MockScheduler::new());
    let nats = TypedNatsClient::new_custom(Arc::new(MockNatsClient::new()));

    let config = crate::config::ApiConfig {
        database_url: "postgres://localhost/dummy".to_string(),
        nats_url: "nats://localhost:4222".to_string(),
        jwt_secret: "secret".to_string(),
        master_key: "0".repeat(64),
        router_addr: "localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        ..Default::default()
    };

    let ctx = ApiContext {
        user_repo: user_repo.clone(),
        tenant_repo: Arc::new(crate::domain::MockTenantRepository::new()),
        app_repo: app_repo.clone(),
        database_repo: database_repo.clone(),
        github_repo: github_repo.clone(),
        volume_repo: volume_repo.clone(),
        scheduler: scheduler.clone(),
        nats: nats.clone(),
        db: db.clone(),
        config: Arc::new(config),
        jwt_secret: "secret".to_string(),
        master_key: "0".repeat(64),
    };

    AppState {
        ctx,
        user_repo,
        tenant_repo: Arc::new(crate::domain::MockTenantRepository::new()),
        app_repo,
        database_repo,
        github_repo,
        volume_repo,
        scheduler,
        nats,
        router_addr: "localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        api_db: db,
        jwt_secret: "secret".to_string(),
        master_key: "0".repeat(64),
        deployment_events,
        workspace_events,
        mesh_status,
        acme_email: "test@example.com".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        active_deployment_flows: Arc::new(dashmap::DashSet::new()),
    }
}
