use futures::StreamExt;
use mikrom_proto::agent::VmLogPayload;
use mikrom_proto::router::RouterConfigUpdate;
use prost::Message;
use std::env;

async fn connect_nats_or_skip() -> Option<async_nats::Client> {
    let nats_url =
        env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    match async_nats::connect(&nats_url).await {
        Ok(client) => Some(client),
        Err(err) => {
            eprintln!(
                "Skipping NATS protobuf test: failed to connect to {}: {}",
                nats_url, err
            );
            None
        },
    }
}

#[tokio::test]
async fn test_nats_protobuf_serialization_router() {
    let Some(client) = connect_nats_or_skip().await else {
        return;
    };

    let subject = "test.mikrom.router.config_updated";
    let mut sub = client.subscribe(subject).await.unwrap();

    // Simulate what mikrom-api does
    let update = RouterConfigUpdate {
        hostname: "example.com".to_string(),
        target_urls: vec!["http://[fd00::1]:8080".to_string()],
        timestamp: chrono::Utc::now().timestamp(),
    };

    let payload = update.encode_to_vec();
    client.publish(subject, payload.into()).await.unwrap();

    // Simulate what mikrom-router does
    if let Some(msg) = tokio::time::timeout(std::time::Duration::from_secs(2), sub.next())
        .await
        .expect("Timeout waiting for router update")
    {
        let decoded = RouterConfigUpdate::decode(&msg.payload[..])
            .expect("Failed to decode RouterConfigUpdate");
        assert_eq!(decoded.hostname, "example.com");
        assert_eq!(
            decoded.target_urls,
            vec!["http://[fd00::1]:8080".to_string()]
        );
    } else {
        panic!("No message received");
    }
}

#[tokio::test]
async fn test_nats_protobuf_serialization_logs() {
    let Some(client) = connect_nats_or_skip().await else {
        return;
    };

    let vm_id = "test-vm-123";
    let subject = format!("test.mikrom.logs.{}", vm_id);
    let mut sub = client.subscribe(subject.clone()).await.unwrap();

    // Simulate what mikrom-agent does
    let now = chrono::Utc::now().timestamp();
    let log_entry = VmLogPayload {
        line: "Hello from microVM".to_string(),
        timestamp: now,
    };

    let payload = log_entry.encode_to_vec();
    client.publish(subject, payload.into()).await.unwrap();

    // Simulate what a consumer would do
    if let Some(msg) = tokio::time::timeout(std::time::Duration::from_secs(2), sub.next())
        .await
        .expect("Timeout waiting for log entry")
    {
        let decoded =
            VmLogPayload::decode(&msg.payload[..]).expect("Failed to decode VmLogPayload");
        assert_eq!(decoded.line, "Hello from microVM");
        assert_eq!(decoded.timestamp, now);
    } else {
        panic!("No message received");
    }
}
