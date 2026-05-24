use async_nats::Client;
use std::env;

pub mod integration;

/// Returns a connected NATS client for testing, or `None` when NATS is unavailable.
#[allow(dead_code)]
pub async fn get_nats_client_or_skip() -> Option<Client> {
    dotenvy::dotenv().ok();
    let nats_url =
        env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    match async_nats::connect(nats_url).await {
        Ok(client) => Some(client),
        Err(err) => {
            eprintln!("Skipping API test: failed to connect to NATS: {err}");
            None
        },
    }
}

/// Returns a connected NATS client for tests that require a live broker.
#[allow(dead_code)]
pub async fn get_nats_client() -> Client {
    get_nats_client_or_skip()
        .await
        .expect("Failed to connect to NATS for testing")
}
