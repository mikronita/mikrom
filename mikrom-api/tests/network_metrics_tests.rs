use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use mockall::predicate::eq;
use tower::ServiceExt;
use uuid::Uuid;

use mikrom_api::AppState;
use mikrom_api::auth::jwt::create_token;
use mikrom_api::create_app;
use mikrom_api::domain::app::{App, Deployment};
use mikrom_api::domain::github::MockGithubRepository;
use mikrom_api::domain::user::{MockUserRepository, UserRole};
use mikrom_api::domain::{MockAppRepository, MockScheduler, MockTenantRepository};

const JWT_SECRET: &str = "test-secret";

fn nats_integration_enabled() -> bool {
    if std::env::var("MIKROM_RUN_NATS_TESTS").is_err() {
        println!("Skipping NATS test: set MIKROM_RUN_NATS_TESTS=1 to run it");
        return false;
    }

    true
}

async fn connect_nats_or_skip(test_name: &str) -> Option<async_nats::Client> {
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());

    match async_nats::connect(nats_url).await {
        Ok(client) => Some(client),
        Err(err) => {
            eprintln!("skipping {test_name}: unable to connect to NATS: {err}");
            None
        },
    }
}

async fn build_state(app_repo: MockAppRepository, nats_client: async_nats::Client) -> AppState {
    AppState {
        ctx: mikrom_api::application::ApiContext::default(),
        user_repo: Arc::new(MockUserRepository::new()),
        tenant_repo: Arc::new(MockTenantRepository::new()),
        app_repo: Arc::new(app_repo),
        database_repo: Arc::new(mikrom_api::domain::MockDatabaseRepository::new()),
        github_repo: Arc::new(MockGithubRepository::default()),
        volume_repo: Arc::new(mikrom_api::domain::MockVolumeRepository::new()),
        scheduler: Arc::new(MockScheduler::new()),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        api_db: sqlx::PgPool::connect_lazy("postgres://localhost/dummy").unwrap(),
        jwt_secret: JWT_SECRET.into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(100).0,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        mesh_status: tokio::sync::watch::channel(
            mikrom_api::application::vms::MeshStatus::default(),
        )
        .0,
        acme_email: "admin@mikrom.example.com".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        apps_domain: "apps.mikrom.example.com".to_string(),
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    }
}

#[tokio::test]
#[ignore = "requires a stable NATS scheduler fixture"]
async fn test_active_deployments_endpoint_responds() {
    if !nats_integration_enabled() {
        return;
    }

    let Some(nats_client) = connect_nats_or_skip("test_active_deployments_endpoint_responds").await
    else {
        return;
    };

    let tenant_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let dep_id = Uuid::new_v4();
    let job_id = "job-1".to_string();

    let mut mock_app_repo = MockAppRepository::new();
    let job_id_for_deps = job_id.clone();
    mock_app_repo
        .expect_get_app()
        .with(eq(app_id))
        .returning(move |_| {
            Ok(Some(App {
                id: app_id,
                name: "test-app".to_string(),
                tenant_id,
                active_deployment_id: Some(dep_id),
                desired_replicas: 1,
                ..Default::default()
            }))
        });
    mock_app_repo
        .expect_list_deployments_by_user()
        .with(eq(Some(tenant_id)))
        .returning(move |_| {
            Ok(vec![Deployment {
                id: dep_id,
                app_id,
                tenant_id,
                status: "RUNNING".to_string(),
                job_id: Some(job_id_for_deps.clone()),
                ..Default::default()
            }])
        });

    let nats_clone = nats_client.clone();
    let job_id_for_responder = job_id.clone();
    tokio::spawn(async move {
        use futures::StreamExt;
        use mikrom_proto::scheduler::{AppInfo, ListAppsResponse};
        use prost::Message;

        let mut sub = nats_clone
            .subscribe("mikrom.scheduler.list_apps")
            .await
            .unwrap();
        while let Some(msg) = sub.next().await {
            if let Some(reply) = msg.reply {
                let response = ListAppsResponse {
                    apps: vec![AppInfo {
                        job_id: job_id_for_responder.clone(),
                        deployment_id: dep_id.to_string(),
                        app_id: app_id.to_string(),
                        tenant_id: tenant_id.to_string(),
                        app_name: "test-app".to_string(),
                        status: mikrom_proto::scheduler::DeployStatus::Running as i32,
                        ipv6_address: "fd00::1".to_string(),
                        tx_bytes: 11,
                        rx_bytes: 22,
                        ..Default::default()
                    }],
                };
                let mut buf = Vec::new();
                response.encode(&mut buf).unwrap();
                let _ = nats_clone.publish(reply, buf.into()).await;
            }
        }
    });

    let state = build_state(mock_app_repo, nats_client).await;
    let router = create_app(state);
    let token = create_token(
        &tenant_id.to_string(),
        "test@test.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/deployments/active")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 10000)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let deployments = json.as_array().expect("response should be an array");
    assert!(!deployments.is_empty());
    let dep = &deployments[0];
    assert_eq!(deployments.len(), 1);
    assert_eq!(dep.get("job_id").and_then(|v| v.as_str()), Some("job-1"));
    assert_eq!(dep.get("tx_bytes").and_then(|v| v.as_u64()), Some(11));
    assert_eq!(dep.get("rx_bytes").and_then(|v| v.as_u64()), Some(22));
}

#[tokio::test]
#[ignore = "requires a stable NATS scheduler fixture"]
async fn test_active_deployments_endpoint_filters_non_running_jobs() {
    if !nats_integration_enabled() {
        return;
    }

    let Some(nats_client) =
        connect_nats_or_skip("test_active_deployments_endpoint_filters_non_running_jobs").await
    else {
        return;
    };

    let tenant_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let dep_id = Uuid::new_v4();
    let running_job_id = "job-running".to_string();
    let paused_job_id = "job-paused".to_string();

    let mut mock_app_repo = MockAppRepository::new();
    let running_job_id_for_deps = running_job_id.clone();
    mock_app_repo
        .expect_get_app()
        .with(eq(app_id))
        .returning(move |_| {
            Ok(Some(App {
                id: app_id,
                name: "test-app".to_string(),
                tenant_id,
                active_deployment_id: Some(dep_id),
                desired_replicas: 1,
                ..Default::default()
            }))
        });
    mock_app_repo
        .expect_list_deployments_by_user()
        .with(eq(Some(tenant_id)))
        .returning(move |_| {
            Ok(vec![Deployment {
                id: dep_id,
                app_id,
                tenant_id,
                status: "RUNNING".to_string(),
                job_id: Some(running_job_id_for_deps.clone()),
                ..Default::default()
            }])
        });

    let nats_clone = nats_client.clone();
    let running_job_id_for_responder = running_job_id.clone();
    tokio::spawn(async move {
        use futures::StreamExt;
        use mikrom_proto::scheduler::{AppInfo, ListAppsResponse};
        use prost::Message;

        let mut sub = nats_clone
            .subscribe("mikrom.scheduler.list_apps")
            .await
            .unwrap();
        while let Some(msg) = sub.next().await {
            if let Some(reply) = msg.reply {
                let response = ListAppsResponse {
                    apps: vec![
                        AppInfo {
                            job_id: running_job_id_for_responder.clone(),
                            deployment_id: dep_id.to_string(),
                            app_id: app_id.to_string(),
                            tenant_id: tenant_id.to_string(),
                            app_name: "test-app".to_string(),
                            status: mikrom_proto::scheduler::DeployStatus::Running as i32,
                            ipv6_address: "fd00::1".to_string(),
                            tx_bytes: 11,
                            rx_bytes: 22,
                            ..Default::default()
                        },
                        AppInfo {
                            job_id: paused_job_id.clone(),
                            deployment_id: Uuid::new_v4().to_string(),
                            app_id: app_id.to_string(),
                            tenant_id: tenant_id.to_string(),
                            app_name: "test-app".to_string(),
                            status: mikrom_proto::scheduler::DeployStatus::Paused as i32,
                            ipv6_address: "fd00::2".to_string(),
                            tx_bytes: 33,
                            rx_bytes: 44,
                            ..Default::default()
                        },
                    ],
                };
                let mut buf = Vec::new();
                response.encode(&mut buf).unwrap();
                let _ = nats_clone.publish(reply, buf.into()).await;
            }
        }
    });

    let state = build_state(mock_app_repo, nats_client).await;
    let router = create_app(state);
    let token = create_token(
        &tenant_id.to_string(),
        "test@test.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/deployments/active")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 10000)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let deployments = json.as_array().expect("response should be an array");
    assert_eq!(deployments.len(), 1);
    assert_eq!(
        deployments[0].get("job_id").and_then(|v| v.as_str()),
        Some("job-running")
    );
}

#[tokio::test]
#[ignore = "requires a stable watch receiver in test state"]
async fn test_mesh_status_endpoint_responds() {
    let Some(nats_client) = connect_nats_or_skip("test_mesh_status_endpoint_responds").await else {
        return;
    };

    let tenant_id = Uuid::new_v4();
    let app_repo = MockAppRepository::new();
    let state = build_state(app_repo, nats_client).await;
    state
        .mesh_status
        .send(mikrom_api::application::vms::MeshStatus::default())
        .unwrap();

    let router = create_app(state);
    let token = create_token(
        &tenant_id.to_string(),
        "test@test.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/networking/mesh")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
