use prost::Message;
use mikrom_proto::router::RouterConfigUpdate;

#[tokio::test]
async fn test_publish_route() -> anyhow::Result<()> {
    let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
    let client = async_nats::connect(nats_url).await?;

    let update = RouterConfigUpdate {
        hostname: "test.mikrom.local".to_string(),
        target_url: Some("127.0.0.1:9000".to_string()),
        timestamp: 0,
    };

    let mut payload = Vec::new();
    update.encode(&mut payload)?;

    client.publish(mikrom_proto::subjects::ROUTER_CONFIG_UPDATED, payload.into()).await?;
    client.flush().await?;

    println!("Published update for test.mikrom.local -> 127.0.0.1:9000");
    Ok(())
}
