use futures::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
    println!("Connecting to NATS at {}", nats_url);
    let client = async_nats::connect(nats_url).await?;

    println!("Subscribing to mikrom.>");
    let mut sub = client.subscribe("mikrom.>").await?;

    while let Some(msg) = sub.next().await {
        println!("Subject: {}", msg.subject);
        if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&msg.payload) {
            println!("Payload: {}", serde_json::to_string_pretty(&json)?);
        } else {
            println!("Payload (raw): {:?}", msg.payload);
        }
    }

    Ok(())
}
