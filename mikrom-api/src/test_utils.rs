use crate::AppState;
use crate::application::ApiContext;
use crate::application::vms::MeshStatus;
use crate::domain::{
    MockAppRepository, MockDatabaseRepository, MockGithubRepository,
    MockPersonalAccessTokenRepository, MockScheduler, MockUserRepository, MockVolumeRepository,
};
use crate::infrastructure::nats::{MockNatsClient, TypedNatsClient};
use sqlx::{Connection, Executor, PgConnection, PgPool, postgres::PgPoolOptions};
use std::env;
use std::ops::Deref;
use std::sync::{Arc, OnceLock, Weak};

const DEFAULT_TEST_DATABASE_URL: &str =
    "postgres://mikrom:mikrom_password@localhost:5432/mikrom_api_test";

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
            uuid::Uuid::new_v4().simple()
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

                if tokio::time::timeout(std::time::Duration::from_secs(5), cleanup)
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

pub fn create_test_app_state(db: PgPool) -> AppState {
    let (deployment_events, _) = tokio::sync::broadcast::channel(100);
    let (workspace_events, _) = tokio::sync::broadcast::channel(100);
    let (mesh_status, _) = tokio::sync::watch::channel(MeshStatus::default());

    let user_repo = Arc::new(MockUserRepository::new());
    let app_repo = Arc::new(MockAppRepository::new());
    let database_repo = Arc::new(MockDatabaseRepository::new());
    let github_repo = Arc::new(MockGithubRepository::new());
    let volume_repo = Arc::new(MockVolumeRepository::new());
    let personal_access_token_repo = Arc::new(MockPersonalAccessTokenRepository::new());
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
    let mut plan_tier_repo = crate::domain::MockPlanTierRepository::new();
    plan_tier_repo.expect_get_default_tier().returning(|| {
        Ok(crate::domain::plan_tier::PlanTier {
            id: uuid::Uuid::new_v4(),
            polar_product_id: None,
            tier_slug: crate::domain::plan_tier::TierSlug::Free,
            name: "Free".to_string(),
            max_apps: 3,
            max_databases: 3,
            max_volumes: 3,
            max_vcpus_total: 2,
            max_memory_mb_total: 1024,
            max_storage_gb_total: 5,
            max_deployments_per_app: 10,
            max_team_members: 1,
            autoscaling_allowed: false,
            custom_domains: false,
            trial_days: 0,
            is_default: true,
            sort_order: 0,
            created_at: chrono::Utc::now(),
        })
    });
    plan_tier_repo.expect_get_tenant_tier().returning(|_| {
        Ok(crate::domain::plan_tier::PlanTier {
            id: uuid::Uuid::new_v4(),
            polar_product_id: None,
            tier_slug: crate::domain::plan_tier::TierSlug::Free,
            name: "Free".to_string(),
            max_apps: 3,
            max_databases: 3,
            max_volumes: 3,
            max_vcpus_total: 2,
            max_memory_mb_total: 1024,
            max_storage_gb_total: 5,
            max_deployments_per_app: 10,
            max_team_members: 1,
            autoscaling_allowed: false,
            custom_domains: false,
            trial_days: 0,
            is_default: true,
            sort_order: 0,
            created_at: chrono::Utc::now(),
        })
    });
    plan_tier_repo
        .expect_assign_to_tenant()
        .returning(|_, _| Ok(()));

    let mut tenant_usage_repo = crate::domain::MockTenantUsageRepository::new();
    tenant_usage_repo
        .expect_get_or_create()
        .returning(|tenant_id| {
            Ok(crate::domain::plan_tier::TenantUsage {
                tenant_id,
                apps_count: 0,
                databases_count: 0,
                volumes_count: 0,
                vcpus_total: 0,
                memory_mb_total: 0,
                storage_gb_total: 0,
                deployments_count: 0,
                bandwidth_gb_billed: 0,
                updated_at: chrono::Utc::now(),
            })
        });
    tenant_usage_repo
        .expect_increment_apps()
        .returning(|_, _, _, _, _| Ok(()));
    tenant_usage_repo
        .expect_decrement_apps()
        .returning(|_, _, _, _| Ok(()));
    tenant_usage_repo
        .expect_increment_databases()
        .returning(|_, _| Ok(()));
    tenant_usage_repo
        .expect_decrement_databases()
        .returning(|_| Ok(()));
    tenant_usage_repo
        .expect_increment_volumes()
        .returning(|_, _, _| Ok(()));
    tenant_usage_repo
        .expect_decrement_volumes()
        .returning(|_, _| Ok(()));
    tenant_usage_repo
        .expect_increment_deployments()
        .returning(|_, _| Ok(()));
    tenant_usage_repo
        .expect_decrement_deployments()
        .returning(|_| Ok(()));

    let ctx = ApiContext {
        user_repo: user_repo.clone(),
        tenant_repo: Arc::new(crate::domain::MockTenantRepository::new()),
        app_repo: app_repo.clone(),
        database_repo: database_repo.clone(),
        github_repo: github_repo.clone(),
        volume_repo: volume_repo.clone(),
        plan_tier_repo: Arc::new(plan_tier_repo),
        tenant_usage_repo: Arc::new(tenant_usage_repo),
        personal_access_token_repo: personal_access_token_repo.clone(),
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
        apps_domain: "apps.mikrom.example.com".to_string(),
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        active_deployment_flows: Arc::new(dashmap::DashSet::new()),
    }
}
