use crate::config::ApiConfig;
use crate::domain::MockDatabaseRepository;
use crate::domain::{
    AppRepository, DatabaseRepository, GithubRepository, MockAppRepository, MockGithubRepository,
    MockPersonalAccessTokenRepository, MockPlanTierRepository, MockScheduler, MockTenantRepository,
    MockTenantUsageRepository, MockUserRepository, MockVolumeRepository,
    PersonalAccessTokenRepository, PlanTierRepository, Scheduler, TenantRepository,
    TenantUsageRepository, UserRepository, VolumeRepository,
};
use crate::nats::TypedNatsClient;
use std::sync::Arc;

/// Dependency bag shared across the API application layer.
///
/// Analogous to `AppContext` in `mikrom-scheduler`, this struct holds
/// references to all external dependencies (repositories, scheduler, NATS)
/// so that application services and HTTP handlers receive a single,
/// cloneable context instead of a 25-field `AppState` god object.
#[derive(Clone)]
pub struct ApiContext {
    pub user_repo: Arc<dyn UserRepository>,
    pub tenant_repo: Arc<dyn TenantRepository>,
    pub app_repo: Arc<dyn AppRepository>,
    pub database_repo: Arc<dyn DatabaseRepository>,
    pub github_repo: Arc<dyn GithubRepository>,
    pub volume_repo: Arc<dyn VolumeRepository>,
    pub plan_tier_repo: Arc<dyn PlanTierRepository>,
    pub tenant_usage_repo: Arc<dyn TenantUsageRepository>,
    pub personal_access_token_repo: Arc<dyn PersonalAccessTokenRepository>,
    pub scheduler: Arc<dyn Scheduler>,
    pub nats: TypedNatsClient,
    pub db: sqlx::PgPool,
    pub config: Arc<ApiConfig>,
    pub jwt_secret: String,
    pub master_key: String,
}

impl Default for ApiContext {
    fn default() -> Self {
        let config = ApiConfig::default();
        let jwt_secret = config.jwt_secret.clone();
        let master_key = config.master_key.clone();

        Self {
            user_repo: Arc::new(MockUserRepository::new()),
            tenant_repo: Arc::new(MockTenantRepository::new()),
            app_repo: Arc::new(MockAppRepository::new()),
            database_repo: Arc::new(MockDatabaseRepository::new()),
            github_repo: Arc::new(MockGithubRepository::new()),
            volume_repo: Arc::new(MockVolumeRepository::new()),
            plan_tier_repo: Arc::new(MockPlanTierRepository::new()),
            tenant_usage_repo: Arc::new(MockTenantUsageRepository::new()),
            personal_access_token_repo: Arc::new(MockPersonalAccessTokenRepository::new()),
            scheduler: Arc::new(MockScheduler::new()),
            nats: TypedNatsClient::default(),
            db: sqlx::PgPool::connect_lazy("postgres://localhost/test").expect("Valid lazy pool"),
            config: Arc::new(config),
            jwt_secret,
            master_key,
        }
    }
}

impl ApiContext {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        user_repo: Arc<dyn UserRepository>,
        tenant_repo: Arc<dyn TenantRepository>,
        app_repo: Arc<dyn AppRepository>,
        database_repo: Arc<dyn DatabaseRepository>,
        github_repo: Arc<dyn GithubRepository>,
        volume_repo: Arc<dyn VolumeRepository>,
        plan_tier_repo: Arc<dyn PlanTierRepository>,
        tenant_usage_repo: Arc<dyn TenantUsageRepository>,
        personal_access_token_repo: Arc<dyn PersonalAccessTokenRepository>,
        scheduler: Arc<dyn Scheduler>,
        nats: TypedNatsClient,
        db: sqlx::PgPool,
        config: ApiConfig,
    ) -> Self {
        let jwt_secret = config.jwt_secret.clone();
        let master_key = config.master_key.clone();
        Self {
            user_repo,
            tenant_repo,
            app_repo,
            database_repo,
            github_repo,
            volume_repo,
            plan_tier_repo,
            tenant_usage_repo,
            personal_access_token_repo,
            scheduler,
            nats,
            db,
            config: Arc::new(config),
            jwt_secret,
            master_key,
        }
    }
}
