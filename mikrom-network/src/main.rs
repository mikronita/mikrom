use anyhow::Context;
use mikrom_network::{FileWireGuardKeyStore, KeyManager, MeshOrchestrator, WireGuardManager};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Setup logging
    tracing_subscriber::fmt::init();

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
    let nats_client = async_nats::connect(nats_url).await?;

    // 5. Run Mesh Orchestrator
    let orchestrator = MeshOrchestrator::new(wg_manager, nats_client);

    tracing::info!("mikrom-network node starting for host {}", host_id);
    orchestrator
        .run(host_id, priv_key, advertise_addr)
        .await
        .map_err(|e| anyhow::anyhow!("Mesh orchestrator error: {e}"))?;

    Ok(())
}
