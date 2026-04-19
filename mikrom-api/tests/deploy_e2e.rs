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
use tower::ServiceExt;

use mikrom_agent::server::AgentServer;
use mikrom_api::auth::jwt::create_token;
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
    async fn create(&self, _: NewUser) -> Result<sqlx::types::Uuid, DbError> {
        Ok(sqlx::types::Uuid::new_v4())
    }
    async fn count_by_email(&self, _: &str) -> Result<i64, DbError> {
        Ok(0)
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
    // `with_scheduler_addr` avoids touching process-global env vars.
    let agent = AgentServer::with_scheduler_addr(
        "e2e-agent-1".to_string(),
        "e2e-node".to_string(),
        "127.0.0.1".to_string(),
        scheduler_url.clone(),
    );
    let agent_addr: SocketAddr = format!("127.0.0.1:{agent_port}").parse().unwrap();
    tokio::spawn(async move {
        agent.serve(agent_addr, false).await.unwrap();
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
    let agent = AgentServer::with_scheduler_addr(
        "e2e-agent-http".to_string(),
        "e2e-http-node".to_string(),
        "127.0.0.1".to_string(),
        scheduler_url.clone(),
    );
    let agent_addr: SocketAddr = format!("127.0.0.1:{agent_port}").parse().unwrap();
    tokio::spawn(async move {
        agent.serve(agent_addr, false).await.unwrap();
    });
    wait_for_tcp(agent_port).await;

    // Wait for the agent to register with the scheduler.
    tokio::time::sleep(Duration::from_secs(2)).await;

    // ── build the API router ──────────────────────────────────────────────────
    let state = AppState {
        user_repo: Arc::new(NoopRepo),
        scheduler_client: None,
        scheduler_config: mikrom_api::scheduler::SchedulerConfig {
            addr: scheduler_url.clone(),
            use_tls: false,
            certs_dir: None,
        },
        jwt_secret: E2E_JWT_SECRET.to_string(),
        master_key: "e2e-key".into(),
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
