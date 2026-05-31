use mikrom_api::AppState;
use mikrom_api::domain::MockAppRepository;
use mikrom_api::domain::MockScheduler;
use mikrom_api::domain::MockTenantRepository;
use mikrom_api::domain::MockUserRepository;
use mikrom_api::domain::MockVolumeRepository;
use mikrom_api::nats::{NatsClient, TypedNatsClient};
use std::sync::Arc;

struct DummyNats;

#[async_trait::async_trait]
impl NatsClient for DummyNats {
    async fn request_raw(&self, _subject: String, _payload: Vec<u8>) -> anyhow::Result<Vec<u8>> {
        Err(anyhow::anyhow!("unexpected request"))
    }

    async fn publish_raw(&self, _subject: String, _payload: Vec<u8>) -> anyhow::Result<()> {
        Ok(())
    }

    async fn subscribe_raw(&self, _subject: String) -> anyhow::Result<async_nats::Subscriber> {
        Err(anyhow::anyhow!("unexpected subscribe"))
    }
}

#[allow(clippy::field_reassign_with_default)]
#[tokio::test]
async fn reconcile_routes_with_no_apps_is_ok() {
    let mut app_repo = MockAppRepository::new();
    app_repo
        .expect_list_apps_by_tenant()
        .returning(|_| Ok(vec![]));

    let mut state = AppState::default();
    state.app_repo = Arc::new(app_repo);
    state.ctx.app_repo = state.app_repo.clone();
    state.user_repo = Arc::new(MockUserRepository::new());
    state.ctx.user_repo = state.user_repo.clone();
    state.tenant_repo = Arc::new(MockTenantRepository::new());
    state.ctx.tenant_repo = state.tenant_repo.clone();
    state.database_repo = Arc::new(mikrom_api::domain::MockDatabaseRepository::new());
    state.ctx.database_repo = state.database_repo.clone();
    state.volume_repo = Arc::new(MockVolumeRepository::new());
    state.ctx.volume_repo = state.volume_repo.clone();
    state.scheduler = Arc::new(MockScheduler::new());
    state.ctx.scheduler = state.scheduler.clone();
    state.nats = TypedNatsClient::new_custom(Arc::new(DummyNats));
    state.ctx.nats = state.nats.clone();

    state.reconcile_routes().await.unwrap();
}
