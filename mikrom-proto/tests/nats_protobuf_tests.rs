use futures::StreamExt;
use mikrom_proto::agent::VmLogPayload;
use mikrom_proto::router::RouterConfigUpdate;
use prost::Message;
use std::env;

#[tokio::test]
async fn test_nats_protobuf_serialization_router() {
    let nats_url = env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
    let client = async_nats::connect(&nats_url)
        .await
        .expect("Failed to connect to NATS");

    let subject = "test.mikrom.router.config_updated";
    let mut sub = client.subscribe(subject).await.unwrap();

    // Simulate what mikrom-api does
    let update = RouterConfigUpdate {
        hostname: "example.com".to_string(),
        target_url: Some("http://10.0.0.1:8080".to_string()),
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
        assert_eq!(decoded.target_url, Some("http://10.0.0.1:8080".to_string()));
    } else {
        panic!("No message received");
    }
}

#[tokio::test]
async fn test_nats_protobuf_serialization_logs() {
    let nats_url = env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
    let client = async_nats::connect(&nats_url)
        .await
        .expect("Failed to connect to NATS");

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
