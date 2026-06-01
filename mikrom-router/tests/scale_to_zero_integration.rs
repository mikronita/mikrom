#![cfg(feature = "scale-to-zero-e2e")]

use axum::http::StatusCode;
use futures_util::StreamExt;
use mikrom_api::NatsScheduler;
use mikrom_api::application::vms::MeshStatus;
use mikrom_api::create_app;
use mikrom_api::domain::user::UserRepository;
use mikrom_api::domain::{
    AppRepository, MockDatabaseRepository, MockGithubRepository, MockVolumeRepository,
    TenantRepository,
};
use mikrom_api::domain::{CpuCores, MemoryMb, Port};
use mikrom_api::infrastructure::db::{
    PostgresAppRepository, PostgresTenantRepository, PostgresUserRepository,
};
use mikrom_api::test_utils::TestDb as ApiTestDb;
use mikrom_proto::router::{RouterConfigAck, RouterConfigUpdate};
use mikrom_proto::subjects;
use mikrom_scheduler::application::{AppService, SchedulerRuntimeConfig};
use mikrom_scheduler::domain::{
    AppConfig, AppId, AppRepository as _, DeploymentId, HostId, Job, JobId, JobRepository as _,
    JobStatus, TenantId, VmConfig, VmId,
};
use mikrom_scheduler::infrastructure::db::{PgJobRepository, PgWorkerRepository};
use mikrom_scheduler::infrastructure::nats::NatsEventLoop;
use mikrom_scheduler::server::SchedulerServer;
use prost::Message;
use rustls::crypto::ring::default_provider;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tower::util::ServiceExt;
#[path = "support/scale_to_zero.rs"]
mod scale_to_zero_support;
use scale_to_zero_support::*;

#[tokio::test]
#[allow(clippy::too_many_lines)]
#[allow(clippy::large_futures)]
async fn test_integration_scale_to_zero_and_restore_reuses_same_job() {
    let _ = default_provider().install_default();

    let Some(env) = setup_test_env(100, true).await else {
        eprintln!("skipping router scale-to-zero e2e test: network bind unavailable");
        return;
    };

    let db = ApiTestDb::new().await;
    let pool = db.pool().clone();
    sqlx::query(
        r"
        ALTER TABLE apps
        ADD COLUMN IF NOT EXISTS vpc_ipv6_prefix VARCHAR NOT NULL DEFAULT '';
        ",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r"
        ALTER TABLE apps
        ADD COLUMN IF NOT EXISTS hostname VARCHAR NOT NULL DEFAULT '';
        ",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r"
        ALTER TABLE apps
        ADD COLUMN IF NOT EXISTS last_router_traffic_at BIGINT NOT NULL DEFAULT 0;
        ",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r"
        ALTER TABLE apps
        ADD COLUMN IF NOT EXISTS last_scaled_to_zero_at BIGINT NOT NULL DEFAULT 0;
        ",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query("DROP TABLE IF EXISTS workers CASCADE")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        r"
        CREATE TABLE workers (
            id VARCHAR PRIMARY KEY,
            hostname VARCHAR NOT NULL,
            ip_address VARCHAR NOT NULL DEFAULT '',
            advertise_address VARCHAR NOT NULL DEFAULT '',
            wireguard_pubkey VARCHAR,
            wireguard_ip VARCHAR,
            wireguard_port INTEGER NOT NULL DEFAULT 51820,
            metrics JSONB,
            status VARCHAR NOT NULL DEFAULT 'Online',
            supported_hypervisors INTEGER[] NOT NULL DEFAULT '{}'::integer[],
            last_heartbeat BIGINT NOT NULL,
            registered_at BIGINT NOT NULL
        )
        ",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r"
        CREATE TABLE IF NOT EXISTS jobs (
            job_id VARCHAR PRIMARY KEY,
            app_id VARCHAR NOT NULL,
            app_name VARCHAR NOT NULL,
            image VARCHAR NOT NULL,
            tenant_id VARCHAR NOT NULL,
            status VARCHAR NOT NULL,
            host_id VARCHAR REFERENCES workers(id) ON DELETE SET NULL,
            vm_id VARCHAR,
            vcpus INTEGER NOT NULL,
            memory_mib BIGINT NOT NULL,
            disk_mib BIGINT NOT NULL,
            port INTEGER NOT NULL,
            env_vars JSONB NOT NULL DEFAULT '{}'::jsonb,
            created_at BIGINT NOT NULL,
            deployment_id VARCHAR,
            health_check_path TEXT DEFAULT '/',
            ipv6_address VARCHAR(45),
            ipv6_gateway VARCHAR(45),
            hypervisor INTEGER NOT NULL DEFAULT 0,
            workload_type INTEGER NOT NULL DEFAULT 0,
            scheduled_at BIGINT,
            started_at BIGINT,
            stopped_at BIGINT,
            error_message TEXT
        )
        ",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_jobs_app_id ON jobs(app_id)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_jobs_tenant_id ON jobs(tenant_id)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs(status)")
        .execute(&pool)
        .await
        .unwrap();

    let now = chrono::Utc::now().timestamp();
    let worker_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        r"
        INSERT INTO workers (
            id, hostname, ip_address, wireguard_pubkey, advertise_address,
            wireguard_ip, wireguard_port, status, last_heartbeat, registered_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        ON CONFLICT (id) DO UPDATE SET
            hostname = EXCLUDED.hostname,
            ip_address = EXCLUDED.ip_address,
            wireguard_pubkey = EXCLUDED.wireguard_pubkey,
            advertise_address = EXCLUDED.advertise_address,
            wireguard_ip = EXCLUDED.wireguard_ip,
            wireguard_port = EXCLUDED.wireguard_port,
            status = EXCLUDED.status,
            last_heartbeat = EXCLUDED.last_heartbeat,
            registered_at = EXCLUDED.registered_at
        ",
    )
    .bind(&worker_id)
    .bind("router-e2e-worker")
    .bind("127.0.0.1")
    .bind("test-wireguard-pubkey")
    .bind("127.0.0.1")
    .bind("10.0.0.1")
    .bind(51820_i32)
    .bind("Online")
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();

    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();
    let router_config_updates = nats_client
        .subscribe(mikrom_proto::subjects::ROUTER_CONFIG_UPDATED)
        .await
        .unwrap();
    let router_nats = nats_client.clone();
    let _router_config_handle = tokio::spawn(async move {
        let mut updates = router_config_updates;
        while let Some(msg) = updates.next().await {
            if RouterConfigUpdate::decode(&msg.payload[..]).is_ok()
                && let Some(reply) = msg.reply
            {
                let response = RouterConfigAck {
                    success: true,
                    message: String::new(),
                };
                let mut buf = Vec::new();
                if response.encode(&mut buf).is_ok() {
                    let _ = router_nats.publish(reply, buf.into()).await;
                }
            }
        }
    });

    let scheduler_app_repo = Arc::new(MemoryAppRepository::new(pool.clone()));
    let scheduler_job_repo = Arc::new(PgJobRepository::new(pool.clone()));
    let scheduler_worker_repo = Arc::new(PgWorkerRepository::new(pool.clone()));
    let agent_client = Arc::new(RecordingAgentClient::default());

    let app_service = Arc::new(AppService::new(
        scheduler_job_repo.clone(),
        scheduler_app_repo.clone(),
        scheduler_worker_repo.clone(),
        agent_client.clone(),
        nats_client.clone(),
        pool.clone(),
        SchedulerRuntimeConfig {
            router_idle_timeout_secs: 900,
            worker_stale_threshold_secs: 60,
            restore_retry_backoff_secs: 3600,
        },
    ));
    let scheduler_server = SchedulerServer::new(app_service.clone(), None);
    let scheduler_event_loop = NatsEventLoop::new(scheduler_server, nats_client.clone());
    let _scheduler_handle = tokio::spawn(async move {
        let _ = scheduler_event_loop.run().await;
    });

    let user_repo = Arc::new(PostgresUserRepository::new(pool.clone()));
    let api_app_repo = Arc::new(PostgresAppRepository::new(
        pool.clone(),
        "test-key".to_string(),
    ));
    let tenant_repo = Arc::new(PostgresTenantRepository::new(pool.clone()));
    let mut api_state = mikrom_api::AppState {
        ctx: mikrom_api::application::ApiContext::default(),
        user_repo: user_repo.clone(),
        tenant_repo: tenant_repo.clone(),
        app_repo: api_app_repo.clone(),
        database_repo: Arc::new(MockDatabaseRepository::new()),
        volume_repo: Arc::new(MockVolumeRepository::new()),
        github_repo: Arc::new(MockGithubRepository::default()),
        scheduler: Arc::new(NatsScheduler::new(mikrom_api::nats::TypedNatsClient::new(
            nats_client.clone(),
        ))),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client.clone()),
        router_addr: env.proxy_url.clone(),
        frontend_url: "http://localhost:3000".to_string(),
        api_db: pool.clone(),
        jwt_secret: "test-secret".to_string(),
        master_key: "test-key".to_string(),
        deployment_events: tokio::sync::broadcast::channel(16).0,
        workspace_events: tokio::sync::broadcast::channel(16).0,
        mesh_status: tokio::sync::watch::channel(MeshStatus::default()).0,
        acme_email: "admin@mikrom.spluca.org".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        active_deployment_flows: Arc::new(dashmap::DashSet::new()),
    };
    api_state.ctx.user_repo = api_state.user_repo.clone();
    api_state.ctx.tenant_repo = api_state.tenant_repo.clone();
    api_state.ctx.app_repo = api_state.app_repo.clone();
    api_state.ctx.database_repo = api_state.database_repo.clone();
    api_state.ctx.volume_repo = api_state.volume_repo.clone();
    api_state.ctx.github_repo = api_state.github_repo.clone();
    api_state.ctx.scheduler = api_state.scheduler.clone();
    api_state.ctx.nats = api_state.nats.clone();
    api_state.ctx.db = api_state.api_db.clone();
    let api = create_app(api_state);

    let email = format!("e2e_{}@example.com", uuid::Uuid::new_v4());
    let password = "password123";

    let register_resp = api
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/auth/register")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({"email": email, "password": password}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(register_resp.status(), StatusCode::CREATED);

    let login_resp = api
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/auth/login")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({"email": email, "password": password}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login_resp.status(), StatusCode::OK);
    let login_body = axum::body::to_bytes(login_resp.into_body(), 4096)
        .await
        .unwrap();
    let login_json: serde_json::Value = serde_json::from_slice(&login_body).unwrap();
    let token = login_json["token"].as_str().unwrap().to_string();
    let registered_user = user_repo.find_by_email(&email).await.unwrap().unwrap();
    let tenant = tenant_repo
        .list_by_user(registered_user.id)
        .await
        .unwrap()
        .into_iter()
        .next()
        .expect("registration should create a default tenant");

    let app_name = format!("e2e-{}", uuid::Uuid::new_v4().simple());
    let upstream_port = env.upstream_addr.port();

    let create_resp = api
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/apps")
                .header("Content-Type", "application/json")
                .header("Authorization", format!("Bearer {token}"))
                .header("x-mikrom-tenant-id", tenant.tenant_id.clone())
                .body(axum::body::Body::from(
                    serde_json::json!({
                        "name": app_name,
                        "git_url": "https://example.com/repo.git",
                        "port": upstream_port,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_resp.status(), StatusCode::CREATED);
    let create_body = axum::body::to_bytes(create_resp.into_body(), 4096)
        .await
        .unwrap();
    let create_json: serde_json::Value = serde_json::from_slice(&create_body).unwrap();
    let hostname = create_json["hostname"].as_str().unwrap().to_string();

    let app_record = api_app_repo
        .get_app_by_name(&app_name)
        .await
        .unwrap()
        .unwrap();

    scheduler_app_repo
        .upsert(AppConfig {
            id: AppId::from(app_record.id.to_string()),
            tenant_id: TenantId::from(app_record.tenant_id.to_string()),
            vpc_ipv6_prefix: String::new(),
            hostname: hostname.clone(),
            desired_replicas: 1,
            min_replicas: 1,
            max_replicas: 1,
            autoscaling_enabled: false,
            cpu_threshold: 80.0,
            mem_threshold: 80.0,
            last_router_traffic_at: 0,
            last_scaled_to_zero_at: 0,
            restore_retry_after_at: 0,
        })
        .await;

    let deployment = api_app_repo
        .create_deployment(mikrom_api::domain::NewDeployment {
            app_id: app_record.id,
            user_id: registered_user.id,
            tenant_id: app_record.tenant_id.to_string(),
            vcpus: CpuCores::new(1).unwrap(),
            memory_mib: MemoryMb::new(128).unwrap(),
            disk_mib: 512,
            port: Port::new(u32::from(upstream_port)).unwrap(),
            env_vars: std::collections::HashMap::new(),
            trigger_source: "manual".to_string(),
            git_commit_hash: Some("abc1234".to_string()),
            git_commit_message: Some("e2e deployment".to_string()),
            git_branch: Some("main".to_string()),
            hypervisor: 0,
        })
        .await
        .unwrap();

    let job_id = deployment.id.to_string();
    let mut job = Job::new(
        JobId::from(job_id.clone()),
        AppId::from(app_record.id.to_string()),
        app_record.name.clone(),
        "demo:latest".to_string(),
        VmConfig {
            vcpus: 1,
            memory_mib: 128,
            disk_mib: 512,
            port: u32::from(upstream_port),
            env: std::collections::HashMap::new(),
            ipv6_address: Some("::1".to_string()),
            ipv6_gateway: None,
            volumes: vec![],
            health_check_path: "/".to_string(),
            hypervisor: mikrom_scheduler::domain::job::HypervisorType::Firecracker,
            workload_type: mikrom_scheduler::domain::job::WorkloadType::App,
        },
        TenantId::from(app_record.tenant_id.to_string()),
        Some(DeploymentId::from(deployment.id.to_string())),
    );
    job.status = JobStatus::Running;
    job.host_id = Some(HostId::from(worker_id.clone()));
    job.vm_id = Some(VmId::from("router-e2e-vm".to_string()));
    let now = chrono::Utc::now().timestamp();
    job.scheduled_at = Some(now);
    job.started_at = Some(now);
    scheduler_job_repo.add_job(job).await.unwrap();

    api_app_repo
        .update_deployment(
            deployment.id,
            mikrom_api::domain::UpdateDeploymentParams {
                status: Some("RUNNING".to_string()),
                job_id: Some(job_id.clone()),
                ipv6_address: Some("::1".to_string()),
                image_tag: Some("demo:latest".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let activate_resp = api
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/apps/{}/deployments/{}/activate",
                    app_name, deployment.id
                ))
                .header("Authorization", format!("Bearer {token}"))
                .header("x-mikrom-tenant-id", tenant.tenant_id.clone())
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(activate_resp.status(), StatusCode::OK);

    scheduler_app_repo
        .update_app_config(AppConfig {
            id: AppId::from(app_record.id.to_string()),
            tenant_id: TenantId::from(app_record.tenant_id.to_string()),
            vpc_ipv6_prefix: String::new(),
            hostname: hostname.clone(),
            desired_replicas: 1,
            min_replicas: 0,
            max_replicas: 1,
            autoscaling_enabled: false,
            cpu_threshold: 80.0,
            mem_threshold: 80.0,
            last_router_traffic_at: now - 1000,
            last_scaled_to_zero_at: 0,
            restore_retry_after_at: 0,
        })
        .await
        .unwrap();

    app_service.reconcile_apps().await.unwrap();

    let apps_resp = api
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/v1/apps")
                .header("Authorization", format!("Bearer {token}"))
                .header("x-mikrom-tenant-id", tenant.tenant_id.clone())
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(apps_resp.status(), StatusCode::OK);
    let apps_body = axum::body::to_bytes(apps_resp.into_body(), 4096)
        .await
        .unwrap();
    let apps_json: serde_json::Value = serde_json::from_slice(&apps_body).unwrap();
    let app_entry = apps_json
        .as_array()
        .and_then(|apps| apps.iter().find(|item| item["name"] == app_name))
        .expect("expected created app in list");
    assert_eq!(app_entry["scale_state"], "warming_up");

    let traffic_event = mikrom_proto::router::RouterTrafficEvent {
        hostname: hostname.clone(),
        router_id: "router-test".to_string(),
        timestamp: chrono::Utc::now().timestamp(),
    };
    let mut traffic_buf = Vec::new();
    traffic_event.encode(&mut traffic_buf).unwrap();
    nats_client
        .publish(subjects::ROUTER_TRAFFIC_EVENT, traffic_buf.into())
        .await
        .unwrap();

    let mut restored = false;
    for _ in 0..40 {
        if let Some(job) = scheduler_job_repo.get_job(&job_id).await.unwrap()
            && job.status == JobStatus::Running
        {
            restored = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    assert!(
        restored,
        "expected paused job to resume after router traffic"
    );
    assert_eq!(agent_client.resumes.load(Ordering::SeqCst), 1);
    assert_eq!(agent_client.starts.load(Ordering::SeqCst), 0);

    let apps_resp = api
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/v1/apps")
                .header("Authorization", format!("Bearer {token}"))
                .header("x-mikrom-tenant-id", tenant.tenant_id.clone())
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(apps_resp.status(), StatusCode::OK);
    let apps_body = axum::body::to_bytes(apps_resp.into_body(), 4096)
        .await
        .unwrap();
    let apps_json: serde_json::Value = serde_json::from_slice(&apps_body).unwrap();
    let app_entry = apps_json
        .as_array()
        .and_then(|apps| apps.iter().find(|item| item["name"] == app_name))
        .expect("expected created app in list");
    assert_eq!(app_entry["scale_state"], "active");
}

#[tokio::test]
#[allow(clippy::large_futures)]
async fn test_router_proxy_forwards_requests_and_publishes_traffic_events() {
    let Some(env) = setup_test_env(100, true).await else {
        eprintln!("skipping router proxy e2e test: network bind unavailable");
        return;
    };

    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();
    let mut traffic_sub = nats_client
        .subscribe(subjects::ROUTER_TRAFFIC_EVENT)
        .await
        .unwrap();

    let client = reqwest::Client::new();
    let proxy_res = client
        .get(format!("{}/", env.proxy_url))
        .header("Host", "localhost")
        .send()
        .await
        .expect("Failed to send request to router proxy");
    assert_eq!(proxy_res.status(), StatusCode::OK);

    let body = proxy_res.text().await.unwrap();
    assert!(body.contains("host: localhost"));

    let event = tokio::time::timeout(std::time::Duration::from_secs(5), traffic_sub.next())
        .await
        .expect("expected router traffic event")
        .expect("traffic subscriber closed");
    let traffic = mikrom_proto::router::RouterTrafficEvent::decode(&event.payload[..]).unwrap();
    assert_eq!(traffic.hostname, "localhost");
}
