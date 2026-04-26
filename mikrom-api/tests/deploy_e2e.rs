/// End-to-end integration tests for the full deploy flow.
///
/// These tests spin up real in-process services (no Docker, no KVM required):
///   - mikrom-scheduler gRPC server on a random port
///   - mikrom-agent    gRPC server on a random port (stub Firecracker)
///
/// Two test levels:
///   1. `test_scheduler_agent_grpc_e2e` — gRPC path only (scheduler → agent)
///   2. `test_http_api_deploy_e2e`      — full HTTP path (API → scheduler → agent)
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{body::Body, http::Request};
use tower::{Service, ServiceExt};

use mikrom_agent::firecracker::{FirecrackerConfig, FirecrackerManager};
use mikrom_agent::server::AgentServer;
use mikrom_api::auth::jwt::create_token;
use mikrom_api::repositories::PostgresAppRepository;
use mikrom_api::repositories::user_repository::{DbError, NewUser, User, UserRepository};
use mikrom_api::{AppState, create_app};
use mikrom_proto::scheduler::{DeployRequest, SchedulerServiceClient};
use mikrom_scheduler::server::SchedulerServer;

// ── helpers ───────────────────────────────────────────────────────────────────

/// Bind on port 0 and return the assigned port; the listener is dropped immediately
/// so the port is free by the time the service binds it.
async fn free_port() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    l.local_addr().unwrap().port()
}

/// Block until a TCP connection to `port` succeeds, or panic after 5 s.
async fn wait_for_tcp(port: u16) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .is_ok()
        {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "service on port {port} did not become ready within 5 s"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

// ── no-op user repository (no DB needed) ─────────────────────────────────────

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
        id: sqlx::types::Uuid,
        _: Option<String>,
        _: Option<String>,
    ) -> Result<User, DbError> {
        Ok(User {
            id,
            email: "noop@example.com".to_string(),
            password_hash: "".to_string(),
            role: mikrom_api::repositories::user_repository::UserRole::User,
            first_name: None,
            last_name: None,
        })
    }
}

const E2E_JWT_SECRET: &str = "e2e-test-secret-do-not-use-in-prod";

// ── test 1: gRPC path only ────────────────────────────────────────────────────

/// Start a real scheduler and a real agent, wait for the agent to register,
/// then deploy an app directly through the scheduler gRPC API.
///
/// The agent uses stub `FirecrackerManager` (FC_KERNEL_PATH is not set in CI),
/// so no KVM is required.
#[tokio::test]
async fn test_scheduler_agent_grpc_e2e() {
    let scheduler_port = free_port().await;
    let agent_port = free_port().await;
    let scheduler_url = format!("http://127.0.0.1:{scheduler_port}");

    // ── start scheduler ───────────────────────────────────────────────────────
    let sched_addr: SocketAddr = format!("127.0.0.1:{scheduler_port}").parse().unwrap();
    tokio::spawn(async move {
        SchedulerServer::new(None)
            .unwrap()
            .serve(sched_addr)
            .await
            .unwrap();
    });
    wait_for_tcp(scheduler_port).await;

    // ── start agent ───────────────────────────────────────────────────────────
    // `with_manager` avoids touching process-global env vars and uses stub mode.
    let agent_config = mikrom_agent::config::AgentConfig {
        host_id: "e2e-agent-1".to_string(),
        scheduler_addr: scheduler_url.clone(),
        use_tls: false,
        agent_port,
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

    // The agent waits 1 s before its first registration attempt; give it plenty of time.
    tokio::time::sleep(Duration::from_secs(10)).await;

    // ── deploy via gRPC ───────────────────────────────────────────────────────
    let mut client = SchedulerServiceClient::connect(scheduler_url)
        .await
        .expect("failed to connect to scheduler");

    let response = client
        .deploy_app(DeployRequest {
            app_id: "e2e-app-1".to_string(),
            app_name: "e2e-test-app".to_string(),
            image: "nginx:latest".to_string(),
            config: None,
            user_id: "test-user".to_string(),
        })
        .await
        .expect("deploy_app RPC failed")
        .into_inner();

    // The agent has registered → a worker is available → job is Scheduled or Running.
    assert!(
        response.status == mikrom_scheduler::JobStatus::Scheduled as i32
            || response.status == mikrom_scheduler::JobStatus::Running as i32,
        "expected Scheduled (2) or Running (3), got status={} message='{}'",
        response.status,
        response.message
    );
    assert_eq!(
        response.host_id, "e2e-agent-1",
        "job should be assigned to e2e-agent-1"
    );
    assert!(
        !response.vm_id.is_empty(),
        "a vm_id should have been assigned"
    );
    assert!(
        !response.job_id.is_empty(),
        "a job_id should have been returned"
    );
}

// ── test 2: full HTTP path ────────────────────────────────────────────────────

/// Start a real scheduler and agent, then exercise the full HTTP path:
///   POST /deploy  →  mikrom-api  →  scheduler gRPC  →  agent gRPC
///
/// Uses a `NoopRepo` for auth (no PostgreSQL required) and a fixed JWT secret.
#[tokio::test]
async fn test_http_api_deploy_e2e() {
    let scheduler_port = free_port().await;
    let agent_port = free_port().await;
    let scheduler_url = format!("http://127.0.0.1:{scheduler_port}");

    // ── start scheduler ───────────────────────────────────────────────────────
    let sched_addr: SocketAddr = format!("127.0.0.1:{scheduler_port}").parse().unwrap();
    tokio::spawn(async move {
        SchedulerServer::new(None)
            .unwrap()
            .serve(sched_addr)
            .await
            .unwrap();
    });
    wait_for_tcp(scheduler_port).await;
    // ── start agent ───────────────────────────────────────────────────────────
    let agent_config = mikrom_agent::config::AgentConfig {
        host_id: "e2e-agent-http".to_string(),
        scheduler_addr: scheduler_url.clone(),
        use_tls: false,
        agent_port,
        bridge_ip: "10.0.0.1/8".to_string(),
        certs_dir: "/certs/agent".to_string(),
        agent_hostname: Some("e2e-http-node".to_string()),
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

    // Wait for the agent to register with the scheduler.
    tokio::time::sleep(Duration::from_secs(2)).await;

    // ── build the API router ──────────────────────────────────────────────────
    let db_pool = sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap();
    let app_repo = Arc::new(PostgresAppRepository::new(db_pool));
    let state = AppState {
        user_repo: Arc::new(NoopRepo),
        app_repo,
        scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
        scheduler_config: mikrom_api::scheduler::SchedulerConfig {
            addr: scheduler_url.clone(),
            use_tls: false,
            certs_dir: None,
        },
        builder_addr: "http://localhost:5004".to_string(),
        jwt_secret: E2E_JWT_SECRET.to_string(),
        master_key: "e2e-key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        build_semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
    };
    let app = create_app(state);

    // ── create a valid JWT for the request ────────────────────────────────────
    let token = create_token(
        "user-e2e",
        "e2e@example.com",
        &mikrom_api::repositories::user_repository::UserRole::User,
        E2E_JWT_SECRET,
    )
    .unwrap();

    // ── POST /deploy ──────────────────────────────────────────────────────────
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/deploy")
                .header("Content-Type", "application/json")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::from(
                    r#"{"app_name":"http-e2e-app","image":"nginx:latest"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        axum::http::StatusCode::OK,
        "expected HTTP 200"
    );

    let body_bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    // The scheduler had a worker → status should be "1" (Scheduled), not "error".
    assert_ne!(
        json["status"].as_str().unwrap_or(""),
        "error",
        "expected Scheduled status, got error: {}",
        json["message"].as_str().unwrap_or("")
    );
    assert!(
        !json["job_id"].as_str().unwrap_or("").is_empty(),
        "job_id must be present"
    );
    assert_eq!(
        json["host_id"].as_str().unwrap_or(""),
        "e2e-agent-http",
        "job should be assigned to e2e-agent-http"
    );
    assert!(
        !json["vm_id"].as_str().unwrap_or("").is_empty(),
        "vm_id must be present"
    );
}

#[tokio::test]
async fn test_sse_deployments_events_e2e() {
    let scheduler_port = free_port().await;
    let scheduler_url = format!("http://127.0.0.1:{scheduler_port}");

    // ── start scheduler ───────────────────────────────────────────────────────
    let sched_addr: SocketAddr = format!("127.0.0.1:{scheduler_port}").parse().unwrap();
    let scheduler_server = SchedulerServer::new(None).unwrap();
    let scheduler_handle = scheduler_server.clone();
    tokio::spawn(async move {
        scheduler_handle.serve(sched_addr).await.unwrap();
    });
    wait_for_tcp(scheduler_port).await;

    // ── build the API router ──────────────────────────────────────────────────
    let db_pool = sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap();
    let app_repo = Arc::new(PostgresAppRepository::new(db_pool));
    let state = AppState {
        user_repo: Arc::new(NoopRepo),
        app_repo: app_repo.clone(),
        scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
        scheduler_config: mikrom_api::scheduler::SchedulerConfig {
            addr: scheduler_url.clone(),
            use_tls: false,
            certs_dir: None,
        },
        builder_addr: "http://localhost:5004".to_string(),
        jwt_secret: E2E_JWT_SECRET.to_string(),
        master_key: "e2e-key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        build_semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
    };
    let app = create_app(state);

    // ── create a valid JWT ────────────────────────────────────────────────────
    let user_id = "user-sse-e2e";
    let token = create_token(
        user_id,
        "sse@example.com",
        &mikrom_api::repositories::user_repository::UserRole::User,
        E2E_JWT_SECRET,
    )
    .unwrap();

    // ── Subscribe to SSE /deployments/events ──────────────────────────────────
    // We can't use oneshot because SSE is a stream. We use tower::Service directly.
    let req = Request::builder()
        .method("GET")
        .uri("/deployments/events")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().call(req).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");

    let mut body_stream = response.into_body().into_data_stream();

    // ── Trigger an event in the background ────────────────────────────────────
    let scheduler_handle = scheduler_server.clone();
    let user_id_str = user_id.to_string();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        // Directly add a job to the scheduler to trigger the broadcast
        let job = mikrom_scheduler::job::Job::new(
            "job-sse-1".to_string(),
            "app-1".to_string(),
            "test-app".to_string(),
            "nginx".to_string(),
            mikrom_scheduler::job::VmConfig::default(),
            user_id_str,
        );
        scheduler_handle.scheduler().add_job(job);
    });

    // ── Read from SSE stream ──────────────────────────────────────────────────
    use tokio_stream::StreamExt;
    let first_chunk = body_stream.next().await.unwrap().unwrap();
    let chunk_str = String::from_utf8_lossy(&first_chunk);

    // SSE format is "data: {...}\n\n"
    assert!(chunk_str.contains("data:"));
    assert!(chunk_str.contains("job-sse-1"));
    assert!(chunk_str.contains("test-app"));
}
