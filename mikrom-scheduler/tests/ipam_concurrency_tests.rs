use mikrom_scheduler::scheduler::ipam::Ipam;
use sqlx::PgPool;
use std::collections::HashSet;

#[path = "common_utils.rs"]
mod common_utils;

#[tokio::test]
async fn test_concurrent_ip_allocation() {
    let db = common_utils::TestDb::new().await;
    let pool = db.pool().clone();

    let worker_id = format!("worker-{}", uuid::Uuid::new_v4());
    let bridge_ip = "10.0.0.1/24";

    // Register worker first (needed for foreign key)
    sqlx::query("INSERT INTO workers (id, hostname, ip_address, agent_port, bridge_ip, last_heartbeat, registered_at) VALUES ($1, $2, $3, $4, $5, $6, $7)")
        .bind(&worker_id)
        .bind("test-host")
        .bind("1.2.3.4")
        .bind(5000)
        .bind(bridge_ip)
        .bind(chrono::Utc::now().timestamp())
        .bind(chrono::Utc::now().timestamp())
        .execute(&pool)
        .await
        .expect("failed to register worker");

    let ipam = Ipam::new(pool.clone(), worker_id.clone(), bridge_ip.to_string());

    // Try to allocate 50 IPs concurrently
    let mut tasks = Vec::new();
    for _ in 0..50 {
        let ipam_clone = ipam.clone();
        tasks.push(tokio::spawn(async move { ipam_clone.allocate().await }));
    }

    let mut allocated_ips = HashSet::new();
    for task in tasks {
        let result = task.await.unwrap().expect("IP allocation failed");
        let allocation = result.expect("No IP allocated");

        // Ensure no duplicate IPs were allocated
        assert!(
            allocated_ips.insert(allocation.ip),
            "Duplicate IP allocated!"
        );
    }

    assert_eq!(allocated_ips.len(), 50);

    // Cleanup
    sqlx::query("DELETE FROM workers WHERE id = $1")
        .bind(&worker_id)
        .execute(&pool)
        .await
        .unwrap();
}
