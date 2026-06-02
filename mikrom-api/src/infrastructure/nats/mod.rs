use async_nats::Client;
use prost::Message;
use std::sync::Arc;
use std::time::Duration;

fn is_no_responders_error(err: &anyhow::Error) -> bool {
    err.to_string().to_lowercase().contains("no responders")
}

fn scheduler_unavailable_error(subject: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "scheduler unavailable: no responders on subject: {}",
        subject
    )
}

#[mockall::automock]
#[async_trait::async_trait]
pub trait NatsClient: Send + Sync {
    async fn request_raw(&self, subject: String, payload: Vec<u8>) -> anyhow::Result<Vec<u8>>;
    async fn publish_raw(&self, subject: String, payload: Vec<u8>) -> anyhow::Result<()>;
    async fn subscribe_raw(&self, subject: String) -> anyhow::Result<async_nats::Subscriber>;
}

#[derive(Clone)]
pub struct TypedNatsClient {
    client: Arc<dyn NatsClient>,
    timeout: Duration,
}

struct AsyncNatsClientWrapper(Client);

#[async_trait::async_trait]
impl NatsClient for AsyncNatsClientWrapper {
    async fn request_raw(&self, subject: String, payload: Vec<u8>) -> anyhow::Result<Vec<u8>> {
        let subject_for_error = subject.clone();
        let response =
            self.0
                .request(subject, payload.into())
                .await
                .map_err(|e| match e.kind() {
                    async_nats::RequestErrorKind::NoResponders => {
                        scheduler_unavailable_error(&subject_for_error)
                    },
                    _ => anyhow::anyhow!("NATS request failed: {}", e),
                })?;
        Ok(response.payload.to_vec())
    }

    async fn publish_raw(&self, subject: String, payload: Vec<u8>) -> anyhow::Result<()> {
        self.0
            .publish(subject, payload.into())
            .await
            .map_err(|e| anyhow::anyhow!("NATS publish failed: {}", e))?;
        Ok(())
    }

    async fn subscribe_raw(&self, subject: String) -> anyhow::Result<async_nats::Subscriber> {
        self.0
            .subscribe(subject)
            .await
            .map_err(|e| anyhow::anyhow!("NATS subscribe failed: {}", e))
    }
}

impl Default for TypedNatsClient {
    fn default() -> Self {
        Self {
            client: Arc::new(MockNatsClient::new()),
            timeout: Duration::from_secs(5),
        }
    }
}

impl TypedNatsClient {
    pub fn new(client: Client) -> Self {
        Self {
            client: Arc::new(AsyncNatsClientWrapper(client)),
            timeout: Duration::from_secs(5),
        }
    }

    pub fn new_custom(client: Arc<dyn NatsClient>) -> Self {
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

        let payload =
            tokio::time::timeout(self.timeout, self.client.request_raw(subject.clone(), buf))
                .await
                .map_err(|_| anyhow::anyhow!("NATS request timed out on subject: {}", subject))?
                .map_err(|e| {
                    if is_no_responders_error(&e) {
                        scheduler_unavailable_error(&subject)
                    } else {
                        e
                    }
                })?;

        let res = Res::decode(&payload[..]).map_err(|e| {
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
        self.client.publish_raw(subject.into(), buf).await
    }

    pub async fn subscribe(
        &self,
        subject: impl Into<String>,
    ) -> anyhow::Result<async_nats::Subscriber> {
        self.client.subscribe_raw(subject.into()).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_no_responders_errors_from_text() {
        let err = anyhow::anyhow!("NATS request failed: no responders: no responders");
        assert!(is_no_responders_error(&err));
    }

    #[test]
    fn does_not_flag_unrelated_errors() {
        let err = anyhow::anyhow!("connection closed");
        assert!(!is_no_responders_error(&err));
    }
}
