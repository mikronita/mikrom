use mikrom_agent::firecracker::FirecrackerManager;
use mikrom_agent::firecracker::config::FirecrackerConfig;
use mikrom_agent::server::AgentServer;
use mikrom_api::AppState;
use mikrom_api::repositories::app_repository::MockAppRepository;
use mikrom_api::repositories::user_repository::MockUserRepository;
use mikrom_proto::scheduler::scheduler_service_client::SchedulerServiceClient;
use mikrom_proto::scheduler::{DeployRequest, DeployStatus};
use mikrom_scheduler::server::SchedulerServer;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

/// Returns a port that is currently free.
async fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Busy-wait until a TCP port is open.
async fn wait_for_tcp(port: u16) {
    let addr = format!("127.0.0.1:{port}");
    for _ in 0..50 {
        if tokio::net::TcpStream::connect(&addr).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timeout waiting for port {port}");
}

#[tokio::test]
async fn test_full_deployment_cycle_e2e() {
    let scheduler_port = free_port().await;
    let agent_port = free_port().await;
    let scheduler_url = format!("http://127.0.0.1:{scheduler_port}");

    // ── start scheduler ───────────────────────────────────────────────────────
    let nats_client = async_nats::connect("nats://localhost:4222").await.unwrap();
    let db_pool = sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap();
    let sched_addr: SocketAddr = format!("127.0.0.1:{scheduler_port}").parse().unwrap();
    let nats_client_clone = nats_client.clone();
    let db_pool_clone = db_pool.clone();
    tokio::spawn(async move {
        SchedulerServer::new(db_pool_clone, nats_client_clone, None)
            .unwrap()
            .serve(sched_addr)
            .await
            .unwrap();
    });
    wait_for_tcp(scheduler_port).await;

    // ── start agent ───────────────────────────────────────────────────────────
    let agent_config = mikrom_agent::config::AgentConfig {
        nats_url: "nats://localhost:4222".to_string(),
        host_id: "e2e-agent-1".to_string(),
        scheduler_addr: scheduler_url.clone(),
        agent_port: 0,
        use_tls: false,
        bridge_ip: "10.0.0.1/8".to_string(),
        certs_dir: "/certs/agent".to_string(),
        agent_hostname: Some("e2e-node".to_string()),
    };
    let agent = AgentServer::with_manager(
        agent_config,
        "127.0.0.1".to_string(),
        FirecrackerManager::with_config(FirecrackerConfig::stub()),
    );
    let agent_addr: SocketAddr = format!("127.0.0.1:{agent_port}").parse().unwrap();
    tokio::spawn(async move {
        agent.serve(agent_addr).await.unwrap();
    });
    wait_for_tcp(agent_port).await;

    // Give time for registration
    tokio::time::sleep(Duration::from_secs(2)).await;

    // ── deploy via gRPC ───────────────────────────────────────────────────────
    let mut client = SchedulerServiceClient::connect(scheduler_url)
        .await
        .expect("failed to connect to scheduler");

    let response = client
        .deploy_app(DeployRequest {
            app_id: "test-app".to_string(),
            app_name: "test-app".to_string(),
            image: "nginx".to_string(),
            user_id: "test-user".to_string(),
            deployment_id: "test-deployment".to_string(),
            config: None,
        })
        .await
        .expect("deploy_app failed")
        .into_inner();

    assert_eq!(response.status, DeployStatus::Scheduled as i32);
    assert!(!response.job_id.is_empty());

    // ── verify state ──────────────────────────────────────────────────────────
    tokio::time::sleep(Duration::from_millis(500)).await;
    let status = client
        .get_app_status(mikrom_proto::scheduler::AppStatusRequest {
            job_id: response.job_id,
            user_id: "test-user".to_string(),
        })
        .await
        .expect("get_app_status failed")
        .into_inner();

    assert_eq!(status.status, DeployStatus::Running as i32);
    assert_eq!(status.host_id, "e2e-agent-1");
}

#[tokio::test]
async fn test_api_scheduler_agent_integration() {
    let scheduler_port = free_port().await;
    let agent_port = free_port().await;
    let scheduler_url = format!("http://127.0.0.1:{scheduler_port}");

    let nats_client = async_nats::connect("nats://localhost:4222").await.unwrap();
    let db_pool = sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap();
    let db_pool_clone = db_pool.clone();
    let nats_client_clone = nats_client.clone();
    tokio::spawn(async move {
        SchedulerServer::new(db_pool_clone, nats_client_clone, None)
            .unwrap()
            .serve(format!("127.0.0.1:{scheduler_port}").parse().unwrap())
            .await
            .unwrap();
    });
    wait_for_tcp(scheduler_port).await;

    let agent_config = mikrom_agent::config::AgentConfig {
        nats_url: "nats://localhost:4222".to_string(),
        host_id: "api-e2e-agent".to_string(),
        scheduler_addr: scheduler_url.clone(),
        agent_port: 0,
        use_tls: false,
        bridge_ip: "10.0.0.1/8".to_string(),
        certs_dir: "/certs/agent".to_string(),
        agent_hostname: Some("api-e2e-node".to_string()),
    };
    let agent = AgentServer::with_manager(
        agent_config,
        "127.0.0.1".to_string(),
        FirecrackerManager::with_config(FirecrackerConfig::stub()),
    );
    tokio::spawn(async move {
        agent
            .serve(format!("127.0.0.1:{agent_port}").parse().unwrap())
            .await
            .unwrap();
    });
    wait_for_tcp(agent_port).await;
    tokio::time::sleep(Duration::from_secs(2)).await;

    let state = AppState {
        user_repo: Arc::new(MockUserRepository::new()),
        app_repo: Arc::new(MockAppRepository::new()),
        scheduler: Arc::new(mikrom_api::scheduler::NatsScheduler {
            client: nats_client.clone(),
        }),
        nats_client,
        router_addr: "http://localhost:8080".to_string(),
        jwt_secret: "secret".to_string(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        build_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
    };

    let _app = mikrom_api::create_app(state);
    // ... test continue ...
}
