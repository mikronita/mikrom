use anyhow::Context;
use mikrom_network::{FileWireGuardKeyStore, KeyManager, MeshOrchestrator, WireGuardManager};
use std::sync::Arc;
use std::time::Duration;

fn parse_timeout_env(name: &str, default_secs: u64) -> anyhow::Result<Duration> {
    let secs = std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default_secs);
    Ok(Duration::from_secs(secs.max(1)))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _telemetry =
        mikrom_proto::telemetry::init_telemetry("mikrom-network", env!("CARGO_PKG_VERSION"), None)?;
    mikrom_proto::telemetry::record_service_startup("mikrom-network");

    let host_id = std::env::var("MIKROM_HOST_ID")
        .context("MIKROM_HOST_ID environment variable is required")?;
    let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://[::1]:4222".to_string());
    let data_dir =
        std::env::var("MIKROM_DATA_DIR").unwrap_or_else(|_| "/var/lib/mikrom-network".to_string());
    let wg_port = std::env::var("MIKROM_WG_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(51823);
    let advertise_addr = std::env::var("MIKROM_ADVERTISE_ADDRESS").ok();
    let nats_connect_timeout = parse_timeout_env("MIKROM_NETWORK_NATS_CONNECT_TIMEOUT_SECS", 5)?;

    // 1. Initialize WireGuard Manager
    let wg_manager = Arc::new(WireGuardManager::new("wg-mikrom").with_listen_port(wg_port));

    // 2. Load or generate keys
    let store = FileWireGuardKeyStore;
    let priv_key = KeyManager::load_or_generate_key(&data_dir, &store)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to manage WireGuard keys: {e}"))?;

    // 3. Initialize interface
    wg_manager
        .init(&priv_key, &host_id)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to initialize WireGuard interface: {e}"))?;

    // 4. Connect to NATS
    let nats_client = tokio::time::timeout(nats_connect_timeout, async_nats::connect(nats_url))
        .await
        .context("timeout connecting to NATS")??;

    // 5. Run Mesh Orchestrator
    let orchestrator = MeshOrchestrator::new(wg_manager, nats_client);

    tracing::info!("mikrom-network node starting for host {}", host_id);
    orchestrator
        .run(host_id, priv_key, advertise_addr)
        .await
        .map_err(|e| anyhow::anyhow!("Mesh orchestrator error: {e}"))?;

    Ok(())
}
