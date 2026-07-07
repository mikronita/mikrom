use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use mikrom_api::AppState;
use mikrom_api::application::vms::MeshStatus;
use mikrom_api::auth::jwt::create_token;
use mikrom_api::create_app;
use mikrom_api::domain::app::{App, Deployment};
use mikrom_api::domain::types::Port;
use mikrom_api::domain::user::{MockUserRepository, User, UserRole};
use mikrom_api::domain::{
    MockAppRepository, MockDatabaseRepository, MockScheduler, MockTenantRepository,
    MockVolumeRepository, Tenant, TenantMember,
};
use mockall::predicate::eq;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

const TENANT_SLUG: &str = "abc123";

fn build_state(
    tenant_id: Uuid,
    owner_user_id: Uuid,
) -> (
    AppState,
    Arc<MockAppRepository>,
    Arc<MockUserRepository>,
    Arc<MockTenantRepository>,
) {
    let mut user_repo = MockUserRepository::new();
    user_repo.expect_find_by_id().returning(move |_| {
        Ok(Some(User {
            id: owner_user_id,
            email: "owner@example.com".to_string(),
            password_hash: "hash".to_string(),
            role: UserRole::User,
            first_name: None,
            last_name: None,
            avatar_url: None,
            vpc_ipv6_prefix: Some("fd00::".to_string()),
            totp_secret: None,
            totp_enabled: false,
            deleted_at: None,
        }))
    });

    let mut tenant_repo = MockTenantRepository::new();
    tenant_repo.expect_find_by_slug().returning(move |slug| {
        Ok((slug == TENANT_SLUG).then_some(Tenant {
            id: tenant_id,
            tenant_id: TENANT_SLUG.to_string(),
            name: "Default Project".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }))
    });
    tenant_repo
        .expect_is_member()
        .returning(move |tid, uid| Ok(tid == tenant_id && uid == owner_user_id));
    tenant_repo.expect_get_members().returning(move |_| {
        Ok(vec![TenantMember {
            tenant_id,
            user_id: owner_user_id,
            role: "admin".to_string(),
        }])
    });

    let mut scheduler = MockScheduler::new();
    scheduler
        .expect_update_app_scaling_config()
        .returning(|_| Ok(true));
    scheduler
        .expect_list_apps()
        .returning(|_| Ok(mikrom_proto::scheduler::ListAppsResponse::default()));

    let app_repo = Arc::new(MockAppRepository::new());
    let user_repo = Arc::new(user_repo);
    let tenant_repo = Arc::new(tenant_repo);
    let state = AppState {
        ctx: mikrom_api::application::ApiContext::default(),
        user_repo: user_repo.clone(),
        tenant_repo: tenant_repo.clone(),
        app_repo: app_repo.clone(),
        database_repo: Arc::new(MockDatabaseRepository::new()),
        github_repo: Arc::new(mikrom_api::domain::github::MockGithubRepository::default()),
        volume_repo: Arc::new(MockVolumeRepository::new()),
        scheduler: Arc::new(scheduler),
        nats: mikrom_api::nats::TypedNatsClient::new_custom(Arc::new(
            mikrom_api::nats::MockNatsClient::new(),
        )),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        api_db: sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/dummy")
            .unwrap(),
        jwt_secret: "test-secret".to_string(),
        master_key: "test-master-key".to_string(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        workspace_events: tokio::sync::broadcast::channel(1).0,
        mesh_status: tokio::sync::watch::channel(MeshStatus::default()).0,
        acme_email: "admin@mikrom.spluca.org".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    (state, app_repo, user_repo, tenant_repo)
}

fn auth_header(token: &str) -> String {
    format!("Bearer {token}")
}

#[tokio::test]
#[ignore = "requires a stable integration fixture for tenant membership"]
async fn create_app_handler_creates_app_for_tenant() {
    let tenant_id = Uuid::new_v4();
    let owner_user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let (mut state, _, _, _) = build_state(tenant_id, owner_user_id);

    let expected_name = "test-app".to_string();
    let expected_git_url = "https://github.com/test/repo".to_string();
    let expected_name_for_mock = expected_name.clone();
    let expected_git_url_for_mock = expected_git_url.clone();
    let mut app_repo_mock = MockAppRepository::new();
    app_repo_mock.expect_create_app().returning(move |params| {
        assert_eq!(params.tenant_id, tenant_id);
        assert_eq!(params.name, expected_name_for_mock);
        assert_eq!(params.git_url, expected_git_url_for_mock);
        Ok(App {
            id: app_id,
            name: params.name,
            git_url: params.git_url,
            port: params.port,
            hostname: params.hostname,
            tenant_id: params.tenant_id,
            github_webhook_secret: params.github_webhook_secret,
            github_installation_id: params.github_installation_id,
            github_repo_id: params.github_repo_id,
            github_repo_full_name: params.github_repo_full_name,
            active_deployment_id: None,
            health_check_path: params.health_check_path.unwrap_or_else(|| "/".to_string()),
            drain_timeout: params.drain_timeout.unwrap_or(10),
            desired_replicas: params.desired_replicas.unwrap_or(1),
            min_replicas: params.min_replicas.unwrap_or(0),
            max_replicas: params.max_replicas.unwrap_or(1),
            autoscaling_enabled: params.autoscaling_enabled.unwrap_or(false),
            cpu_threshold: params.cpu_threshold.unwrap_or(80.0),
            mem_threshold: params.mem_threshold.unwrap_or(80.0),
            last_router_traffic_at: 0,
            last_scaled_to_zero_at: 0,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        })
    });
    state.app_repo = Arc::new(app_repo_mock);
    state.ctx.app_repo = state.app_repo.clone();

    let token = create_token(
        &owner_user_id.to_string(),
        "owner@example.com",
        &UserRole::User,
        "test-secret",
    )
    .unwrap();

    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/apps")
                .header("Authorization", auth_header(&token))
                .header("x-mikrom-tenant-id", TENANT_SLUG)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "name": expected_name,
                        "git_url": expected_git_url,
                        "port": 8080,
                        "github_installation_id": null,
                        "github_repo_id": null,
                        "github_repo_full_name": null,
                        "health_check_path": "/",
                        "drain_timeout": 10,
                        "desired_replicas": 1,
                        "min_replicas": 0,
                        "max_replicas": 1,
                        "autoscaling_enabled": false,
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let created: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(created["name"], "test-app");
    assert_eq!(created["tenant_id"], tenant_id.to_string());
    assert_eq!(created["github_webhook_secret"].as_str().unwrap().len(), 32);
}

#[tokio::test]
async fn list_apps_handler_returns_tenant_apps() {
    let tenant_id = Uuid::new_v4();
    let owner_user_id = Uuid::new_v4();
    let (mut state, _, _, _) = build_state(tenant_id, owner_user_id);

    let app = App {
        id: Uuid::new_v4(),
        name: "list-app".to_string(),
        git_url: "https://github.com/test/repo".to_string(),
        port: Port::new(8080).unwrap(),
        hostname: Some("list-app.apps.mikrom.spluca.org".to_string()),
        tenant_id,
        github_webhook_secret: None,
        github_installation_id: None,
        github_repo_id: None,
        github_repo_full_name: None,
        active_deployment_id: None,
        health_check_path: "/".to_string(),
        drain_timeout: 10,
        desired_replicas: 1,
        min_replicas: 0,
        max_replicas: 1,
        autoscaling_enabled: false,
        cpu_threshold: 80.0,
        mem_threshold: 80.0,
        last_router_traffic_at: 0,
        last_scaled_to_zero_at: 0,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let mut app_repo_mock = MockAppRepository::new();
    app_repo_mock
        .expect_list_apps_by_tenant()
        .with(eq(Some(tenant_id)))
        .returning(move |_| Ok(vec![app.clone()]));
    state.app_repo = Arc::new(app_repo_mock);
    state.ctx.app_repo = state.app_repo.clone();

    let token = create_token(
        &owner_user_id.to_string(),
        "owner@example.com",
        &UserRole::User,
        "test-secret",
    )
    .unwrap();

    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/apps")
                .header("Authorization", auth_header(&token))
                .header("x-mikrom-tenant-id", TENANT_SLUG)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let apps: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(apps.as_array().unwrap().len(), 1);
    assert_eq!(apps[0]["name"], "list-app");
}

#[tokio::test]
async fn list_apps_handler_masks_webhook_secret() {
    let tenant_id = Uuid::new_v4();
    let owner_user_id = Uuid::new_v4();
    let (mut state, _, _, _) = build_state(tenant_id, owner_user_id);

    let app = App {
        id: Uuid::new_v4(),
        name: "secret-app".to_string(),
        git_url: "https://github.com/test/repo".to_string(),
        port: Port::new(8080).unwrap(),
        hostname: Some("secret-app.apps.mikrom.spluca.org".to_string()),
        tenant_id,
        github_webhook_secret: Some("super-secret".to_string()),
        github_installation_id: None,
        github_repo_id: None,
        github_repo_full_name: None,
        active_deployment_id: None,
        health_check_path: "/".to_string(),
        drain_timeout: 10,
        desired_replicas: 1,
        min_replicas: 0,
        max_replicas: 1,
        autoscaling_enabled: false,
        cpu_threshold: 80.0,
        mem_threshold: 80.0,
        last_router_traffic_at: 0,
        last_scaled_to_zero_at: 0,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let mut app_repo_mock = MockAppRepository::new();
    app_repo_mock
        .expect_list_apps_by_tenant()
        .with(eq(Some(tenant_id)))
        .returning(move |_| Ok(vec![app.clone()]));
    state.app_repo = Arc::new(app_repo_mock);
    state.ctx.app_repo = state.app_repo.clone();

    let token = create_token(
        &owner_user_id.to_string(),
        "owner@example.com",
        &UserRole::User,
        "test-secret",
    )
    .unwrap();

    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/apps")
                .header("Authorization", auth_header(&token))
                .header("x-mikrom-tenant-id", TENANT_SLUG)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let apps: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(apps.as_array().unwrap().len(), 1);
    assert_eq!(apps[0]["github_webhook_secret"], "********");
}

#[tokio::test]
async fn get_app_secret_handler_returns_raw_secret() {
    let tenant_id = Uuid::new_v4();
    let owner_user_id = Uuid::new_v4();
    let (mut state, _, _, _) = build_state(tenant_id, owner_user_id);

    let app = App {
        id: Uuid::new_v4(),
        name: "secret-app".to_string(),
        git_url: "https://github.com/test/repo".to_string(),
        port: Port::new(8080).unwrap(),
        hostname: Some("secret-app.apps.mikrom.spluca.org".to_string()),
        tenant_id,
        github_webhook_secret: Some("super-secret".to_string()),
        github_installation_id: None,
        github_repo_id: None,
        github_repo_full_name: None,
        active_deployment_id: None,
        health_check_path: "/".to_string(),
        drain_timeout: 10,
        desired_replicas: 1,
        min_replicas: 0,
        max_replicas: 1,
        autoscaling_enabled: false,
        cpu_threshold: 80.0,
        mem_threshold: 80.0,
        last_router_traffic_at: 0,
        last_scaled_to_zero_at: 0,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let mut app_repo_mock = MockAppRepository::new();
    app_repo_mock
        .expect_get_app_by_name()
        .with(eq("secret-app"))
        .returning(move |_| Ok(Some(app.clone())));
    state.app_repo = Arc::new(app_repo_mock);
    state.ctx.app_repo = state.app_repo.clone();

    let token = create_token(
        &owner_user_id.to_string(),
        "owner@example.com",
        &UserRole::User,
        "test-secret",
    )
    .unwrap();

    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/apps/secret-app/secret")
                .header("Authorization", auth_header(&token))
                .header("x-mikrom-tenant-id", TENANT_SLUG)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let secret: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(secret["github_webhook_secret"], "super-secret");
}

#[tokio::test]
async fn update_app_handler_updates_port_and_returns_updated_app() {
    let tenant_id = Uuid::new_v4();
    let owner_user_id = Uuid::new_v4();
    let (mut state, _, _, _) = build_state(tenant_id, owner_user_id);

    let original_app = App {
        id: Uuid::new_v4(),
        name: "port-app".to_string(),
        git_url: "https://github.com/test/repo".to_string(),
        port: Port::new(8080).unwrap(),
        hostname: None,
        tenant_id,
        github_webhook_secret: None,
        github_installation_id: None,
        github_repo_id: None,
        github_repo_full_name: None,
        active_deployment_id: None,
        health_check_path: "/".to_string(),
        drain_timeout: 10,
        desired_replicas: 1,
        min_replicas: 0,
        max_replicas: 1,
        autoscaling_enabled: false,
        cpu_threshold: 80.0,
        mem_threshold: 80.0,
        last_router_traffic_at: 0,
        last_scaled_to_zero_at: 0,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let updated_app = App {
        port: Port::new(3000).unwrap(),
        ..original_app.clone()
    };

    let mut app_repo_mock = MockAppRepository::new();
    app_repo_mock
        .expect_get_app_by_name()
        .with(eq("port-app"))
        .returning({
            let original_app = original_app.clone();
            move |_| Ok(Some(original_app.clone()))
        });
    app_repo_mock
        .expect_update_app_port()
        .with(eq(original_app.id), eq(Port::new(3000).unwrap()))
        .returning(|_, _| Ok(()));
    app_repo_mock
        .expect_get_app()
        .with(eq(original_app.id))
        .returning({
            let updated_app = updated_app.clone();
            move |_| Ok(Some(updated_app.clone()))
        });
    state.app_repo = Arc::new(app_repo_mock);
    state.ctx.app_repo = state.app_repo.clone();

    let token = create_token(
        &owner_user_id.to_string(),
        "owner@example.com",
        &UserRole::User,
        "test-secret",
    )
    .unwrap();

    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/v1/apps/port-app")
                .header("Authorization", auth_header(&token))
                .header("x-mikrom-tenant-id", TENANT_SLUG)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "port": 3000,
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let app: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(app["name"], "port-app");
    assert_eq!(app["port"], 3000);
}

#[tokio::test]
async fn list_deployments_handler_returns_tenant_deployments() {
    let tenant_id = Uuid::new_v4();
    let owner_user_id = Uuid::new_v4();
    let (mut state, _, _, _) = build_state(tenant_id, owner_user_id);

    let app = App {
        id: Uuid::new_v4(),
        name: "deploy-app".to_string(),
        git_url: "https://github.com/test/repo".to_string(),
        port: Port::new(8080).unwrap(),
        hostname: Some("deploy-app.apps.mikrom.spluca.org".to_string()),
        tenant_id,
        github_webhook_secret: None,
        github_installation_id: None,
        github_repo_id: None,
        github_repo_full_name: None,
        active_deployment_id: None,
        health_check_path: "/".to_string(),
        drain_timeout: 10,
        desired_replicas: 1,
        min_replicas: 0,
        max_replicas: 1,
        autoscaling_enabled: false,
        cpu_threshold: 80.0,
        mem_threshold: 80.0,
        last_router_traffic_at: 0,
        last_scaled_to_zero_at: 0,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let deployment = Deployment {
        id: Uuid::new_v4(),
        app_id: app.id,
        tenant_id,
        build_id: Some("build-1".to_string()),
        image_tag: Some("ghcr.io/mikrom/deploy-app:latest".to_string()),
        job_id: Some("job-1".to_string()),
        ipv6_address: Some("fd00::1".to_string()),
        status: "RUNNING".to_string(),
        vcpus: mikrom_api::domain::types::CpuCores::new(1).unwrap(),
        memory_mib: mikrom_api::domain::types::MemoryMb::new(512).unwrap(),
        disk_mib: 1024,
        port: Port::new(8080).unwrap(),
        env_vars: serde_json::json!({"ENV": "value"}),
        git_commit_hash: Some("abc123".to_string()),
        git_commit_message: Some("Deploy app".to_string()),
        git_branch: Some("main".to_string()),
        trigger_source: "manual".to_string(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        hypervisor: 0,
    };
    let expected_app_id = app.id;
    let expected_deployment_id = deployment.id;

    let mut app_repo_mock = MockAppRepository::new();
    app_repo_mock
        .expect_get_app_by_name()
        .with(eq("deploy-app"))
        .returning({
            let app = app.clone();
            move |_| Ok(Some(app.clone()))
        });
    app_repo_mock
        .expect_list_deployments_by_app()
        .with(eq(expected_app_id))
        .returning({
            let deployment = deployment.clone();
            move |_| Ok(vec![deployment.clone()])
        });
    state.app_repo = Arc::new(app_repo_mock);
    state.ctx.app_repo = state.app_repo.clone();

    let token = create_token(
        &owner_user_id.to_string(),
        "owner@example.com",
        &UserRole::User,
        "test-secret",
    )
    .unwrap();

    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/apps/deploy-app/deployments")
                .header("Authorization", auth_header(&token))
                .header("x-mikrom-tenant-id", TENANT_SLUG)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let deployments: Value = serde_json::from_slice(&body).unwrap();
    let deployments = deployments
        .as_array()
        .expect("deployments should be an array");
    assert_eq!(deployments.len(), 1);
    assert_eq!(deployments[0]["id"], expected_deployment_id.to_string());
    assert_eq!(deployments[0]["app_id"], expected_app_id.to_string());
}
