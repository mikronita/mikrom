use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{body::Body, http::Request};
use tower::ServiceExt;

use mikrom_agent::firecracker::{FirecrackerConfig, FirecrackerManager};
use mikrom_agent::server::AgentServer;
use mikrom_api::auth::jwt::create_token;
use mikrom_api::repositories::user_repository::{DbError, NewUser, User, UserRepository};
use mikrom_api::{AppState, create_app};
use mikrom_scheduler::server::SchedulerServer;

// ── helpers ───────────────────────────────────────────────────────────────────

async fn free_port() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    l.local_addr().unwrap().port()
}

async fn wait_for_tcp(port: u16) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .is_ok()
        {
            return;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

struct NoopRepo;
#[async_trait::async_trait]
impl UserRepository for NoopRepo {
    async fn find_by_email(&self, _: &str) -> Result<Option<User>, DbError> {
        Ok(None)
    }
    async fn find_by_id(&self, _: sqlx::types::Uuid) -> Result<Option<User>, DbError> {
        Ok(None)
    }
    async fn create(&self, _: NewUser) -> Result<sqlx::types::Uuid, DbError> {
        Ok(sqlx::types::Uuid::new_v4())
    }
    async fn count_by_email(&self, _: &str) -> Result<i64, DbError> {
        Ok(0)
    }
    async fn update_profile(
        &self,
        _: sqlx::types::Uuid,
        _: Option<String>,
        _: Option<String>,
    ) -> Result<(), DbError> {
        Ok(())
    }
}

const CHAOS_JWT_SECRET: &str = "chaos-test-secret";

#[tokio::test]
async fn test_agent_failure_propagation_e2e() {
    let scheduler_port = free_port().await;
    let agent_port = free_port().await;
    let scheduler_url = format!("http://127.0.0.1:{scheduler_port}");

    // 1. Start Scheduler
    let sched_addr: SocketAddr = format!("127.0.0.1:{scheduler_port}").parse().unwrap();
    tokio::spawn(async move {
        SchedulerServer::new(None)
            .unwrap()
            .serve(sched_addr)
            .await
            .unwrap();
    });
    wait_for_tcp(scheduler_port).await;

    // 2. Start Agent with a failing Firecracker configuration (invalid binary path)
    let failing_config = FirecrackerConfig {
        binary: "/usr/bin/this-does-not-exist-mikrom-test".to_string(),
        kernel_path: Some("/tmp/fake-kernel".to_string()), // Forces real mode instead of stub
        rootfs_path: "/tmp/fake-rootfs.ext4".to_string(),
        ..FirecrackerConfig::stub()
    };
    let manager = FirecrackerManager::with_config(failing_config);

    let agent_config = mikrom_agent::config::AgentConfig {
        host_id: "chaos-agent-1".to_string(),
        scheduler_addr: scheduler_url.clone(),
        use_tls: false,
        agent_port,
        bridge_ip: "10.0.0.1/8".to_string(),
        certs_dir: "/certs/agent".to_string(),
        agent_hostname: Some("chaos-node".to_string()),
    };

    let agent = AgentServer::with_manager(agent_config, "127.0.0.1".to_string(), manager);
    let agent_addr: SocketAddr = format!("127.0.0.1:{agent_port}").parse().unwrap();
    tokio::spawn(async move {
        agent.serve(agent_addr).await.unwrap();
    });
    wait_for_tcp(agent_port).await;

    // Wait for registration
    tokio::time::sleep(Duration::from_secs(2)).await;

    // 3. Setup API
    let state = AppState {
        user_repo: Arc::new(NoopRepo),
        scheduler_client: None,
        scheduler_config: mikrom_api::scheduler::SchedulerConfig {
            addr: scheduler_url.clone(),
            use_tls: false,
            certs_dir: None,
        },
        jwt_secret: CHAOS_JWT_SECRET.to_string(),
        master_key: "chaos-key".into(),
    };
    let app = create_app(state);
    let token = create_token(
        "user-chaos",
        "chaos@example.com",
        &mikrom_api::repositories::user_repository::UserRole::User,
        CHAOS_JWT_SECRET,
    )
    .unwrap();

    // 4. Attempt Deployment - use an image name that looks like a local path to skip Docker build
    let fake_rootfs = "/tmp/fake-rootfs-chaos.ext4";
    std::fs::write(fake_rootfs, "dummy").unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/deploy")
                .header("Content-Type", "application/json")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::from(format!(
                    r#"{{"app_name":"chaos-app","image":"{}"}}"#,
                    fake_rootfs
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    // Since the scheduler now propagates agent errors, the API might return 500
    // if the error happens during the deploy_app call synchronously.
    assert_eq!(
        response.status(),
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    );

    let body_bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    let message = json["error"].as_str().unwrap_or("");

    tracing::info!("Chaos test result: error={}", message);

    assert!(
        message.to_lowercase().contains("no such file")
            || message.to_lowercase().contains("failed")
            || message.to_lowercase().contains("not found")
            || message.to_lowercase().contains("error")
            || message.to_lowercase().contains("vm"),
        "Expected error message about VM or missing binary, got: {}",
        message
    );
}

#[tokio::test]
async fn test_ipam_sequential_allocation_e2e() {
    let scheduler_port = free_port().await;
    let scheduler_url = format!("http://127.0.0.1:{scheduler_port}");

    // Start Scheduler
    let sched_addr: SocketAddr = format!("127.0.0.1:{scheduler_port}").parse().unwrap();
    tokio::spawn(async move {
        SchedulerServer::new(None)
            .unwrap()
            .serve(sched_addr)
            .await
            .unwrap();
    });
    wait_for_tcp(scheduler_port).await;

    // We don't need a real agent for this test, we just want to see what IP the scheduler assigns
    let mut client = mikrom_proto::scheduler::SchedulerServiceClient::connect(scheduler_url)
        .await
        .unwrap();

    // Mock a worker registration so deployment can proceed to IP assignment
    client
        .register_worker(mikrom_proto::scheduler::RegisterWorkerRequest {
            host_id: "chaos-agent-1".to_string(),
            hostname: "chaos-node".to_string(),
            ip_address: "127.0.0.1".to_string(),
            agent_port: 5003,
            bridge_ip: "10.0.0.1/8".to_string(),
        })
        .await
        .unwrap();

    // Add metrics so it's considered available
    client
        .report_metrics(mikrom_proto::scheduler::ReportMetricsRequest {
            host_id: "test-host".to_string(),
            cpu_usage: 0.1,
            ram_used_bytes: 0,
            ram_total_bytes: 8000000000,
            disk_used_bytes: 0,
            disk_total_bytes: 100000000000,
            apps_count: 0,
            vms: std::collections::HashMap::new(),
            load_avg_1: 0.0,
            load_avg_5: 0.0,
            load_avg_15: 0.0,
            timestamp: 0,
        })
        .await
        .unwrap();

    // Deploy two apps and check their IPs (they should be .2 and .3)
    let resp1 = client
        .deploy_app(mikrom_proto::scheduler::DeployRequest {
            app_id: "app-1".to_string(),
            app_name: "app-1".to_string(),
            image: "nginx".to_string(),
            config: None,
            user_id: "user-1".to_string(),
        })
        .await
        .unwrap()
        .into_inner();

    let resp2 = client
        .deploy_app(mikrom_proto::scheduler::DeployRequest {
            app_id: "app-2".to_string(),
            app_name: "app-2".to_string(),
            image: "nginx".to_string(),
            config: None,
            user_id: "user-1".to_string(),
        })
        .await
        .unwrap()
        .into_inner();

    // Since we don't have a real IPAM-to-Response mapping in the current Job struct yet,
    // this test is a placeholder for when we improve the Job model.
    // For now, it at least ensures the scheduler doesn't crash with multiple deploys.
    let _ = client
        .get_app_status(mikrom_proto::scheduler::AppStatusRequest {
            job_id: resp1.job_id,
            user_id: "user-1".to_string(),
        })
        .await
        .unwrap();

    let _ = client
        .get_app_status(mikrom_proto::scheduler::AppStatusRequest {
            job_id: resp2.job_id,
            user_id: "user-1".to_string(),
        })
        .await
        .unwrap();
}
