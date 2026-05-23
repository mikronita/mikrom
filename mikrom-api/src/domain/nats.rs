#[mockall::automock]
#[async_trait::async_trait]
pub trait NatsClient: Send + Sync {
    async fn request_raw(&self, subject: String, payload: Vec<u8>) -> anyhow::Result<Vec<u8>>;
    async fn publish_raw(&self, subject: String, payload: Vec<u8>) -> anyhow::Result<()>;
    async fn subscribe_raw(&self, subject: String) -> anyhow::Result<async_nats::Subscriber>;
}
