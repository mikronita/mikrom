use anyhow::Result;
use mikrom_dns::application::records::DnsRecordStore;
use mikrom_proto::scheduler::{AppInfo, DeployStatus, WorkerHeartbeat};
use prost::Message;
use std::time::Duration;

fn nats_integration_enabled() -> bool {
    if std::env::var("MIKROM_RUN_NATS_TESTS").is_err() {
        println!("Skipping NATS test: set MIKROM_RUN_NATS_TESTS=1 to run it");
        return false;
    }

    true
}

// Helper to simulate a NATS publish for Job Updates
async fn simulate_job_publish(
    nats_client: &async_nats::Client,
    app_name: &str,
    user_id: &str,
    ip: &str,
    status: DeployStatus,
) -> Result<()> {
    let info = AppInfo {
        app_name: app_name.to_string(),
        tenant_id: user_id.to_string(),
        ipv6_address: ip.to_string(),
        status: status as i32,
        ..Default::default()
    };
    let mut buf = Vec::new();
    info.encode(&mut buf)?;
    nats_client
        .publish(
            mikrom_proto::subjects::SCHEDULER_JOB_UPDATES.to_string(),
            buf.into(),
        )
        .await?;
    nats_client.flush().await?;
    Ok(())
}

// Helper to simulate a NATS publish for Worker Heartbeats
async fn simulate_worker_publish(
    nats_client: &async_nats::Client,
    host_id: &str,
    ip: &str,
) -> Result<()> {
    let heartbeat = WorkerHeartbeat {
        host_id: host_id.to_string(),
        wireguard_ip: ip.to_string(),
        ..Default::default()
    };
    let mut buf = Vec::new();
    heartbeat.encode(&mut buf)?;
    nats_client
        .publish(
            mikrom_proto::subjects::SCHEDULER_WORKER_HEARTBEAT.to_string(),
            buf.into(),
        )
        .await?;
    nats_client.flush().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a NATS broker; run with MIKROM_RUN_NATS_TESTS=1 cargo test -p mikrom-dns --test integration -- --ignored"]
async fn test_dns_resolution_lifecycle() -> Result<()> {
    if !nats_integration_enabled() {
        return Ok(());
    }

    let nats_url =
        std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
    let Ok(nats_client) = async_nats::connect(&nats_url).await else {
        eprintln!("Skipping integration test: NATS not found at {nats_url}");
        return Ok(());
    };

    let records = DnsRecordStore::new();

    // 1. Start NATS subscriber
    let subscriber_records = records.clone();
    let dns_config = mikrom_dns::infrastructure::config::DnsConfig::from_env()?;
    tokio::spawn(async move {
        let _ = mikrom_dns::run_nats_subscriber(subscriber_records, &dns_config).await;
    });

    tokio::time::sleep(Duration::from_millis(500)).await;

    // 2. Test User Zone Record
    let app_name = "test-app";
    let user_id = "user-123456";
    let ipv6_user = "fdac:5111:a310:e0bd::1";
    simulate_job_publish(
        &nats_client,
        app_name,
        user_id,
        ipv6_user,
        DeployStatus::Running,
    )
    .await?;

    let user_key = "test-app.user-123456";
    let mut found = false;
    for _ in 0..10 {
        if records.contains_user(user_key) {
            found = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(found, "User record should be present");

    // 3. Test Network Zone Record
    let host_id = "worker-01";
    let ipv6_net = "fd00::1";
    simulate_worker_publish(&nats_client, host_id, ipv6_net).await?;

    let net_key = "worker-01";
    found = false;
    for _ in 0..10 {
        if records.contains_network(net_key) {
            found = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(found, "Network record should be present");

    Ok(())
}
