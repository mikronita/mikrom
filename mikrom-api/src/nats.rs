use async_nats::Client;
use prost::Message;
use std::time::Duration;

#[derive(Clone)]
pub struct TypedNatsClient {
    client: Client,
    timeout: Duration,
}

impl TypedNatsClient {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            timeout: Duration::from_secs(5),
        }
    }

    pub fn with_timeout(&self, timeout: Duration) -> Self {
        Self {
            client: self.client.clone(),
            timeout,
        }
    }

    pub async fn request<Req, Res>(
        &self,
        subject: impl Into<String>,
        request: Req,
    ) -> anyhow::Result<Res>
    where
        Req: Message,
        Res: Message + Default,
    {
        let subject = subject.into();
        let mut buf = Vec::new();
        request.encode(&mut buf)?;

        let response = tokio::time::timeout(
            self.timeout,
            self.client.request(subject.clone(), buf.into()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("NATS request timed out on subject: {}", subject))?
        .map_err(|e| anyhow::anyhow!("NATS request failed on subject {}: {}", subject, e))?;

        let res = Res::decode(&response.payload[..]).map_err(|e| {
            anyhow::anyhow!("Failed to decode NATS response from {}: {}", subject, e)
        })?;
        Ok(res)
    }

    pub async fn publish<Msg>(&self, subject: impl Into<String>, message: Msg) -> anyhow::Result<()>
    where
        Msg: Message,
    {
        let mut buf = Vec::new();
        message.encode(&mut buf)?;
        self.client
            .publish(subject.into(), buf.into())
            .await
            .map_err(|e| anyhow::anyhow!("NATS publish failed: {}", e))?;
        Ok(())
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub async fn subscribe(
        &self,
        subject: impl Into<String>,
    ) -> anyhow::Result<async_nats::Subscriber> {
        self.client
            .subscribe(subject.into())
            .await
            .map_err(|e| anyhow::anyhow!("NATS subscribe failed: {}", e))
    }
}
