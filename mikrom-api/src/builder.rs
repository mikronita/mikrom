use anyhow::Result;
use tonic::transport::Channel;

pub async fn connect(addr: &str) -> Result<Channel> {
    Channel::from_shared(addr.to_string())?
        .connect()
        .await
        .map_err(Into::into)
}
