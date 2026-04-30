use mikrom_scheduler::scheduler::ipam::Ipam;
use sqlx::PgPool;
use std::collections::HashSet;

async fn get_test_pool() -> PgPool {
    let url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://mikrom:mikrom_password@localhost:5432/mikrom_scheduler_test".to_string()
    });
    PgPool::connect(&url)
        .await
        .expect("failed to connect to test db")
}

#[tokio::test]
#[ignore = "requires PostgreSQL"]
async fn test_concurrent_ip_allocation() {
    let pool = get_test_pool().await;

    // Run migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("failed to run migrations");

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
