use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use mikrom_api::AppState;
use mikrom_api::application::vms::MeshStatus;
use mikrom_api::auth::jwt::create_token;
use mikrom_api::create_app;
use mikrom_api::domain::Tenant;
use mikrom_api::domain::app::App;
use mikrom_api::domain::github::MockGithubRepository;
use mikrom_api::domain::types::Port;
use mikrom_api::domain::user::{MockUserRepository, User, UserRole};
use mikrom_api::domain::volume::{Volume, VolumeSnapshot};
use mikrom_api::domain::{
    CreateSnapshotParams, CreateVolumeParams, MockAppRepository, MockDatabaseRepository,
    MockScheduler, MockTenantRepository, MockVolumeRepository, TenantMember,
};
use tower::ServiceExt;
use uuid::Uuid;

struct TestVolumeNats;

#[async_trait::async_trait]
impl mikrom_api::nats::NatsClient for TestVolumeNats {
    async fn request_raw(&self, subject: String, _payload: Vec<u8>) -> anyhow::Result<Vec<u8>> {
        let success = matches!(
            subject.as_str(),
            "mikrom.scheduler.create_volume"
                | "mikrom.scheduler.create_snapshot"
                | "mikrom.scheduler.get_volume_usage"
        );
        if !success {
            return Err(anyhow::anyhow!("unexpected subject: {}", subject));
        }

        let mut buf = Vec::new();
        if subject == "mikrom.scheduler.create_volume" {
            prost::Message::encode(
                &mikrom_proto::scheduler::CreateVolumeResponse {
                    success: true,
                    message: String::new(),
                },
                &mut buf,
            )
            .unwrap();
        } else if subject == "mikrom.scheduler.create_snapshot" {
            prost::Message::encode(
                &mikrom_proto::scheduler::CreateSnapshotResponse {
                    success: true,
                    message: String::new(),
                },
                &mut buf,
            )
            .unwrap();
        } else {
            prost::Message::encode(
                &mikrom_proto::scheduler::GetVolumeUsageResponse {
                    success: true,
                    message: String::new(),
                    provisioned_bytes: 104857600,
                    used_bytes: 20971520,
                },
                &mut buf,
            )
            .unwrap();
        }
        Ok(buf)
    }

    async fn publish_raw(&self, _subject: String, _payload: Vec<u8>) -> anyhow::Result<()> {
        Ok(())
    }

    async fn subscribe_raw(&self, _subject: String) -> anyhow::Result<async_nats::Subscriber> {
        Err(anyhow::anyhow!("unexpected subscribe"))
    }
}

fn build_state(
    tenant_id: Uuid,
    user_id: Uuid,
) -> (
    AppState,
    Arc<MockVolumeRepository>,
    Arc<MockAppRepository>,
    Arc<MockUserRepository>,
) {
    let mut user_repo = MockUserRepository::new();
    user_repo.expect_find_by_id().returning(move |_| {
        Ok(Some(User {
            id: user_id,
            email: "test@example.com".to_string(),
            password_hash: "hash".to_string(),
            avatar_url: None,
            role: UserRole::User,
            first_name: None,
            last_name: None,
            vpc_ipv6_prefix: Some("fd00::".to_string()),
            totp_secret: None,
            totp_enabled: false,
            deleted_at: None,
            email_notifications: true,
            marketing_emails: false,
        }))
    });

    let mut app_repo = MockAppRepository::new();
    app_repo.expect_get_app().returning(move |_| {
        Ok(Some(App {
            id: Uuid::new_v4(),
            name: "app".to_string(),
            git_url: "https://github.com/test/repo".to_string(),
            port: Port::new(8080).unwrap(),
            hostname: Some("app.apps.mikrom.spluca.org".to_string()),
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
        }))
    });

    let mut volume_repo = MockVolumeRepository::new();
    volume_repo.expect_get_volume().returning(move |volume_id| {
        Ok(Some(Volume {
            id: volume_id,
            tenant_id,
            name: "test-vol".to_string(),
            size_mib: 1024,
            pool_name: "test-pool".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }))
    });

    let app_repo = Arc::new(app_repo);
    let user_repo = Arc::new(user_repo);
    let volume_repo = Arc::new(volume_repo);
    let tenant = Tenant {
        id: tenant_id,
        tenant_id: "tenant".to_string(),
        name: "Test Tenant".to_string(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    let tenant_slug = tenant.tenant_id.clone();
    let tenant_id_for_membership = tenant.id;
    let mut tenant_repo = MockTenantRepository::new();
    tenant_repo.expect_find_by_slug().returning(move |slug| {
        if slug == tenant_slug {
            Ok(Some(tenant.clone()))
        } else {
            Ok(None)
        }
    });
    tenant_repo
        .expect_is_member()
        .returning(move |_, user_id| Ok(user_id == tenant_id_for_membership));
    tenant_repo.expect_get_members().returning(move |_| {
        Ok(vec![TenantMember {
            tenant_id,
            user_id,
            role: "admin".to_string(),
        }])
    });

    let mut state = AppState {
        ctx: mikrom_api::application::ApiContext::default(),
        user_repo: user_repo.clone(),
        tenant_repo: Arc::new(tenant_repo),
        app_repo: app_repo.clone(),
        database_repo: Arc::new(MockDatabaseRepository::new()),
        github_repo: Arc::new(MockGithubRepository::default()),
        volume_repo: volume_repo.clone(),
        scheduler: Arc::new(MockScheduler::new()),
        nats: mikrom_api::nats::TypedNatsClient::new_custom(Arc::new(TestVolumeNats)),
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
        acme_email: "admin@mikrom.example.com".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        apps_domain: "apps.mikrom.example.com".to_string(),
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };
    state.ctx.tenant_repo = state.tenant_repo.clone();

    (state, volume_repo, app_repo, user_repo)
}

fn auth_header(token: &str) -> String {
    format!("Bearer {token}")
}

#[tokio::test]
#[ignore = "requires a stable volume response fixture"]
async fn create_volume_handler_creates_volume_for_tenant() {
    let tenant_id = Uuid::new_v4();
    let user_id = tenant_id;
    let volume_id = Uuid::new_v4();
    let (mut state, _, _, _) = build_state(tenant_id, user_id);

    let mut volume_repo_mock = MockVolumeRepository::new();
    volume_repo_mock
        .expect_create_volume()
        .returning(move |params: CreateVolumeParams| {
            assert_eq!(params.user_id, user_id);
            assert_eq!(params.tenant_id, tenant_id);
            assert_eq!(params.name, "test-vol");
            assert_eq!(params.size_mib, 1024);
            Ok(Volume {
                id: volume_id,
                tenant_id,
                name: params.name,
                size_mib: params.size_mib,
                pool_name: params.pool_name,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            })
        });
    state.volume_repo = Arc::new(volume_repo_mock);
    state.ctx.volume_repo = state.volume_repo.clone();

    let token = create_token(
        &user_id.to_string(),
        "test@example.com",
        &UserRole::User,
        "test-secret",
    )
    .unwrap();
    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/volumes")
                .header("x-mikrom-tenant-id", "tenant")
                .header("Authorization", auth_header(&token))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "name": "test-vol",
                        "size_mib": 1024,
                        "pool_name": "test-pool"
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
    let volume: Volume = serde_json::from_slice(&body).unwrap();
    assert_eq!(volume.id, volume_id);
    assert_eq!(volume.tenant_id, tenant_id);
}

#[tokio::test]
async fn create_snapshot_handler_creates_snapshot_for_volume() {
    let tenant_id = Uuid::new_v4();
    let user_id = tenant_id;
    let volume_id = Uuid::new_v4();
    let snapshot_id = Uuid::new_v4();
    let (mut state, _, _, _) = build_state(tenant_id, user_id);

    let mut volume_repo_mock = MockVolumeRepository::new();
    volume_repo_mock.expect_get_volume().returning(move |_| {
        Ok(Some(Volume {
            id: volume_id,
            tenant_id,
            name: "test-vol".to_string(),
            size_mib: 1024,
            pool_name: "test-pool".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }))
    });
    volume_repo_mock
        .expect_create_snapshot()
        .returning(move |params: CreateSnapshotParams| {
            assert_eq!(params.user_id, user_id);
            assert_eq!(params.volume_id, volume_id);
            assert_eq!(params.tenant_id, tenant_id);
            assert_eq!(params.name, "snap-1");
            Ok(VolumeSnapshot {
                id: snapshot_id,
                volume_id: params.volume_id,
                tenant_id: params.tenant_id,
                name: params.name,
                created_at: chrono::Utc::now(),
            })
        });
    volume_repo_mock
        .expect_list_snapshots_by_volume()
        .returning(|_| Ok(vec![]));
    state.volume_repo = Arc::new(volume_repo_mock);
    state.ctx.volume_repo = state.volume_repo.clone();

    let token = create_token(
        &user_id.to_string(),
        "test@example.com",
        &UserRole::User,
        "test-secret",
    )
    .unwrap();
    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/volumes/{volume_id}/snapshots"))
                .header("x-mikrom-tenant-id", "tenant")
                .header("Authorization", auth_header(&token))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "name": "snap-1"
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
    let snapshot: VolumeSnapshot = serde_json::from_slice(&body).unwrap();
    assert_eq!(snapshot.id, snapshot_id);
    assert_eq!(snapshot.tenant_id, tenant_id);
}

#[tokio::test]
async fn list_snapshots_handler_returns_snapshots() {
    let tenant_id = Uuid::new_v4();
    let user_id = tenant_id;
    let volume_id = Uuid::new_v4();
    let (mut state, _, _, _) = build_state(tenant_id, user_id);

    let snapshot = VolumeSnapshot {
        id: Uuid::new_v4(),
        volume_id,
        tenant_id,
        name: "snap-1".to_string(),
        created_at: chrono::Utc::now(),
    };

    let mut volume_repo_mock = MockVolumeRepository::new();
    volume_repo_mock.expect_get_volume().returning(move |_| {
        Ok(Some(Volume {
            id: volume_id,
            tenant_id,
            name: "test-vol".to_string(),
            size_mib: 1024,
            pool_name: "test-pool".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }))
    });
    volume_repo_mock
        .expect_list_snapshots_by_volume()
        .returning(move |_| Ok(vec![snapshot.clone()]));
    state.volume_repo = Arc::new(volume_repo_mock);
    state.ctx.volume_repo = state.volume_repo.clone();

    let token = create_token(
        &user_id.to_string(),
        "test@example.com",
        &UserRole::User,
        "test-secret",
    )
    .unwrap();
    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/volumes/{volume_id}/snapshots"))
                .header("x-mikrom-tenant-id", "tenant")
                .header("Authorization", auth_header(&token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let snapshots: Vec<VolumeSnapshot> = serde_json::from_slice(&body).unwrap();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].name, "snap-1");
}

#[tokio::test]
async fn list_snapshots_handler_rejects_other_tenant() {
    let tenant_id = Uuid::new_v4();
    let other_tenant_id = Uuid::new_v4();
    let user_id = tenant_id;
    let volume_id = Uuid::new_v4();
    let (mut state, _, _, _) = build_state(tenant_id, user_id);

    let mut volume_repo_mock = MockVolumeRepository::new();
    volume_repo_mock.expect_get_volume().returning(move |_| {
        Ok(Some(Volume {
            id: volume_id,
            tenant_id,
            name: "test-vol".to_string(),
            size_mib: 1024,
            pool_name: "test-pool".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }))
    });
    state.volume_repo = Arc::new(volume_repo_mock);
    state.ctx.volume_repo = state.volume_repo.clone();

    let token = create_token(
        &other_tenant_id.to_string(),
        "other@example.com",
        &UserRole::User,
        "test-secret",
    )
    .unwrap();
    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/volumes/{volume_id}/snapshots"))
                .header("x-mikrom-tenant-id", "tenant")
                .header("Authorization", auth_header(&token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn get_volume_usage_handler_returns_usage() {
    let tenant_id = Uuid::new_v4();
    let user_id = tenant_id;
    let volume_id = Uuid::new_v4();
    let (mut state, _, _, _) = build_state(tenant_id, user_id);

    let mut volume_repo_mock = MockVolumeRepository::new();
    volume_repo_mock.expect_get_volume().returning(move |_| {
        Ok(Some(Volume {
            id: volume_id,
            tenant_id,
            name: "test-vol".to_string(),
            size_mib: 1024,
            pool_name: "test-pool".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }))
    });
    state.volume_repo = Arc::new(volume_repo_mock);
    state.ctx.volume_repo = state.volume_repo.clone();

    let token = create_token(
        &user_id.to_string(),
        "test@example.com",
        &UserRole::User,
        "test-secret",
    )
    .unwrap();
    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/volumes/{volume_id}/usage"))
                .header("x-mikrom-tenant-id", "tenant")
                .header("Authorization", auth_header(&token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let usage: mikrom_api::application::volumes::VolumeUsageResponse =
        serde_json::from_slice(&body).unwrap();
    assert_eq!(usage.provisioned_bytes, 104857600);
    assert_eq!(usage.used_bytes, 20971520);
}

#[tokio::test]
async fn get_volume_usage_handler_rejects_other_tenant() {
    let tenant_id = Uuid::new_v4();
    let other_tenant_id = Uuid::new_v4();
    let user_id = tenant_id;
    let volume_id = Uuid::new_v4();
    let (mut state, _, _, _) = build_state(tenant_id, user_id);

    let mut volume_repo_mock = MockVolumeRepository::new();
    volume_repo_mock.expect_get_volume().returning(move |_| {
        Ok(Some(Volume {
            id: volume_id,
            tenant_id,
            name: "test-vol".to_string(),
            size_mib: 1024,
            pool_name: "test-pool".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }))
    });
    state.volume_repo = Arc::new(volume_repo_mock);
    state.ctx.volume_repo = state.volume_repo.clone();

    let token = create_token(
        &other_tenant_id.to_string(),
        "other@example.com",
        &UserRole::User,
        "test-secret",
    )
    .unwrap();
    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/volumes/{volume_id}/usage"))
                .header("x-mikrom-tenant-id", "tenant")
                .header("Authorization", auth_header(&token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
