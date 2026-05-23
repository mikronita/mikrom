use crate::config::ApiConfig;
use crate::domain::{
    AppRepository, GithubRepository, MockAppRepository, MockGithubRepository, MockScheduler,
    MockUserRepository, MockVolumeRepository, Scheduler, UserRepository, VolumeRepository,
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
    pub app_repo: Arc<dyn AppRepository>,
    pub github_repo: Arc<dyn GithubRepository>,
    pub volume_repo: Arc<dyn VolumeRepository>,
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
            app_repo: Arc::new(MockAppRepository::new()),
            github_repo: Arc::new(MockGithubRepository::new()),
            volume_repo: Arc::new(MockVolumeRepository::new()),
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
        app_repo: Arc<dyn AppRepository>,
        github_repo: Arc<dyn GithubRepository>,
        volume_repo: Arc<dyn VolumeRepository>,
        scheduler: Arc<dyn Scheduler>,
        nats: TypedNatsClient,
        db: sqlx::PgPool,
        config: ApiConfig,
    ) -> Self {
        let jwt_secret = config.jwt_secret.clone();
        let master_key = config.master_key.clone();
        Self {
            user_repo,
            app_repo,
            github_repo,
            volume_repo,
            scheduler,
            nats,
            db,
            config: Arc::new(config),
            jwt_secret,
            master_key,
        }
    }
}
