use async_nats::Client;
use std::env;

/// Returns a connected NATS client for testing.
#[allow(dead_code)]
pub async fn get_nats_client() -> Client {
    dotenvy::dotenv().ok();
    let nats_url = env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
    async_nats::connect(nats_url)
        .await
        .expect("Failed to connect to NATS for testing")
}
