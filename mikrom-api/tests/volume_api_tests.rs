mod common;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::Utc;
use futures::StreamExt;
use mockall::predicate::*;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

use mikrom_api::AppState;
use mikrom_api::create_app;
use mikrom_api::domain::MockAppRepository;
use mikrom_api::domain::MockVolumeRepository;
use mikrom_api::domain::github::MockGithubRepository;
use mikrom_api::domain::user::MockUserRepository;
use mikrom_api::domain::volume::Volume;
use mikrom_api::nats::{NatsClient, TypedNatsClient};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestDb {
    pool: sqlx::PgPool,
}

impl TestDb {
    async fn new() -> Self {
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/dummy").unwrap();
        Self { pool }
    }
    fn pool(&self) -> &sqlx::PgPool {
        &self.pool
    }
}

struct RestoreSnapshotNats {
    success: bool,
    message: String,
    request_count: AtomicUsize,
}

#[async_trait::async_trait]
impl NatsClient for RestoreSnapshotNats {
    async fn request_raw(&self, subject: String, _payload: Vec<u8>) -> anyhow::Result<Vec<u8>> {
        assert_eq!(subject, "mikrom.scheduler.restore_snapshot");
        self.request_count.fetch_add(1, Ordering::Relaxed);

        let response = mikrom_proto::scheduler::RestoreSnapshotResponse {
            success: self.success,
            message: self.message.clone(),
        };
        let mut buf = Vec::new();
        prost::Message::encode(&response, &mut buf).unwrap();
        Ok(buf)
    }

    async fn publish_raw(&self, _subject: String, _payload: Vec<u8>) -> anyhow::Result<()> {
        Ok(())
    }

    async fn subscribe_raw(&self, _subject: String) -> anyhow::Result<async_nats::Subscriber> {
        Err(anyhow::anyhow!(
            "unexpected subscribe in restore snapshot test"
        ))
    }
}

fn volume_api_test_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

#[tokio::test]
async fn test_list_volume_snapshots_endpoint() {
    let _guard = volume_api_test_lock().lock().await;
    let mock_user_repo = MockUserRepository::new();
    let mock_app_repo = MockAppRepository::new();
    let mut mock_volume_repo = MockVolumeRepository::new();

    let user_id = Uuid::new_v4();
    let volume_id = Uuid::new_v4();
    let jwt_secret = "test-secret";

    let token = mikrom_api::auth::jwt::create_token(
        &user_id.to_string(),
        "test@example.com",
        &mikrom_api::domain::user::UserRole::User,
        jwt_secret,
    )
    .unwrap();

    mock_volume_repo
        .expect_get_volume()
        .with(eq(volume_id))
        .returning(move |id| {
            Ok(Some(Volume {
                id,
                user_id,
                name: "test-vol".to_string(),
                size_mib: 1024,
                pool_name: "test-pool".to_string(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            }))
        });

    mock_volume_repo
        .expect_list_snapshots_by_volume()
        .with(eq(volume_id))
        .returning(|_| Ok(vec![]));

    let db = TestDb::new().await;
    let Some(nats_client) = common::get_nats_client_or_skip().await else {
        return;
    };

    let state = AppState {
        ctx: mikrom_api::application::ApiContext::default(),
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        database_repo: Arc::new(mikrom_api::domain::MockDatabaseRepository::new()),
        github_repo: Arc::new(MockGithubRepository::default()),
        volume_repo: Arc::new(mock_volume_repo),
        scheduler: Arc::new(mikrom_api::domain::MockScheduler::new()),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client),
        router_addr: "".to_string(),
        frontend_url: "".to_string(),
        jwt_secret: jwt_secret.into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        api_db: db.pool().clone(),
        acme_email: "".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        mesh_status: tokio::sync::watch::channel(
            mikrom_api::application::vms::MeshStatus::default(),
        )
        .0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/volumes/{}/snapshots", volume_id))
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_create_volume_snapshot_endpoint() {
    let _guard = volume_api_test_lock().lock().await;
    let mock_user_repo = MockUserRepository::new();
    let mut mock_app_repo = MockAppRepository::new();
    let mut mock_volume_repo = MockVolumeRepository::new();

    let user_id = Uuid::new_v4();
    let volume_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let jwt_secret = "test-secret";

    let token = mikrom_api::auth::jwt::create_token(
        &user_id.to_string(),
        "test@example.com",
        &mikrom_api::domain::user::UserRole::User,
        jwt_secret,
    )
    .unwrap();

    mock_volume_repo
        .expect_get_volume()
        .with(eq(volume_id))
        .returning(move |id| {
            Ok(Some(Volume {
                id,
                user_id,
                name: "test-vol".to_string(),
                size_mib: 1024,
                pool_name: "test-pool".to_string(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            }))
        });

    mock_app_repo.expect_get_app().returning(move |_| {
        Ok(Some(mikrom_api::domain::app::App {
            id: app_id,
            user_id,
            name: "test-app".to_string(),
            git_url: "".to_string(),
            port: mikrom_api::domain::types::Port::new(80).unwrap(),
            ..mikrom_api::domain::app::App::default()
        }))
    });

    mock_volume_repo
        .expect_create_snapshot()
        .returning(|params| {
            Ok(mikrom_api::domain::volume::VolumeSnapshot {
                id: Uuid::new_v4(),
                volume_id: params.volume_id,
                user_id: params.user_id,
                name: params.name,
                created_at: Utc::now(),
            })
        });

    let db = TestDb::new().await;
    let Some(nats_client) = common::get_nats_client_or_skip().await else {
        return;
    };

    let mut subscriber = nats_client
        .subscribe("mikrom.scheduler.create_snapshot")
        .await
        .unwrap();
    let nats_client_clone = nats_client.clone();
    tokio::spawn(async move {
        if let Some(msg) = subscriber.next().await {
            let resp = mikrom_proto::scheduler::CreateSnapshotResponse {
                success: true,
                message: "".to_string(),
            };
            let mut buf = Vec::new();
            prost::Message::encode(&resp, &mut buf).unwrap();
            nats_client_clone
                .publish(msg.reply.unwrap(), buf.into())
                .await
                .unwrap();
        }
    });

    let state = AppState {
        ctx: mikrom_api::application::ApiContext::default(),
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        database_repo: Arc::new(mikrom_api::domain::MockDatabaseRepository::new()),
        github_repo: Arc::new(MockGithubRepository::default()),
        volume_repo: Arc::new(mock_volume_repo),
        scheduler: Arc::new(mikrom_api::domain::MockScheduler::new()),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client),
        router_addr: "".to_string(),
        frontend_url: "".to_string(),
        jwt_secret: jwt_secret.into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        api_db: db.pool().clone(),
        acme_email: "".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        mesh_status: tokio::sync::watch::channel(
            mikrom_api::application::vms::MeshStatus::default(),
        )
        .0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/volumes/{}/snapshots", volume_id))
                .header("Authorization", format!("Bearer {}", token))
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"name": "snap1"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn test_restore_volume_snapshot_endpoint() {
    let _guard = volume_api_test_lock().lock().await;
    let mock_user_repo = MockUserRepository::new();
    let mock_app_repo = MockAppRepository::new();
    let mut mock_volume_repo = MockVolumeRepository::new();

    let user_id = Uuid::new_v4();
    let volume_id = Uuid::new_v4();
    let jwt_secret = "test-secret";

    let token = mikrom_api::auth::jwt::create_token(
        &user_id.to_string(),
        "test@example.com",
        &mikrom_api::domain::user::UserRole::User,
        jwt_secret,
    )
    .unwrap();

    mock_volume_repo
        .expect_get_volume()
        .with(eq(volume_id))
        .returning(move |id| {
            Ok(Some(Volume {
                id,
                user_id,
                name: "test-vol".to_string(),
                size_mib: 1024,
                pool_name: "test-pool".to_string(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            }))
        });

    let db = TestDb::new().await;
    let nats = TypedNatsClient::new_custom(Arc::new(RestoreSnapshotNats {
        success: true,
        message: "".to_string(),
        request_count: AtomicUsize::new(0),
    }));

    let state = AppState {
        ctx: mikrom_api::application::ApiContext::default(),
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        database_repo: Arc::new(mikrom_api::domain::MockDatabaseRepository::new()),
        github_repo: Arc::new(MockGithubRepository::default()),
        volume_repo: Arc::new(mock_volume_repo),
        scheduler: Arc::new(mikrom_api::domain::MockScheduler::new()),
        nats,
        router_addr: "".to_string(),
        frontend_url: "".to_string(),
        jwt_secret: jwt_secret.into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        api_db: db.pool().clone(),
        acme_email: "".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        mesh_status: tokio::sync::watch::channel(
            mikrom_api::application::vms::MeshStatus::default(),
        )
        .0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/volumes/{}/restore", volume_id))
                .header("Authorization", format!("Bearer {}", token))
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"snapshot_name": "snap1"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_delete_snapshot_endpoint() {
    let _guard = volume_api_test_lock().lock().await;
    let mock_user_repo = MockUserRepository::new();
    let mock_app_repo = MockAppRepository::new();
    let mut mock_volume_repo = MockVolumeRepository::new();

    let user_id = Uuid::new_v4();
    let volume_id = Uuid::new_v4();
    let snapshot_id = Uuid::new_v4();
    let jwt_secret = "test-secret";

    let token = mikrom_api::auth::jwt::create_token(
        &user_id.to_string(),
        "test@example.com",
        &mikrom_api::domain::user::UserRole::User,
        jwt_secret,
    )
    .unwrap();

    mock_volume_repo
        .expect_get_snapshot()
        .with(eq(snapshot_id))
        .returning(move |id| {
            Ok(Some(mikrom_api::domain::volume::VolumeSnapshot {
                id,
                volume_id,
                user_id,
                name: "snap1".to_string(),
                created_at: Utc::now(),
            }))
        });

    mock_volume_repo
        .expect_get_volume()
        .with(eq(volume_id))
        .returning(move |id| {
            Ok(Some(Volume {
                id,
                user_id,
                name: "test-vol".to_string(),
                size_mib: 1024,
                pool_name: "test-pool".to_string(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            }))
        });

    mock_volume_repo
        .expect_delete_snapshot()
        .with(eq(snapshot_id))
        .returning(|_| Ok(true));

    let db = TestDb::new().await;
    let Some(nats_client) = common::get_nats_client_or_skip().await else {
        return;
    };

    let mut subscriber = nats_client
        .subscribe("mikrom.scheduler.delete_snapshot")
        .await
        .unwrap();
    let nats_client_clone = nats_client.clone();
    tokio::spawn(async move {
        if let Some(msg) = subscriber.next().await {
            let resp = mikrom_proto::scheduler::DeleteSnapshotResponse {
                success: true,
                message: "".to_string(),
            };
            let mut buf = Vec::new();
            prost::Message::encode(&resp, &mut buf).unwrap();
            nats_client_clone
                .publish(msg.reply.unwrap(), buf.into())
                .await
                .unwrap();
        }
    });

    let state = AppState {
        ctx: mikrom_api::application::ApiContext::default(),
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        database_repo: Arc::new(mikrom_api::domain::MockDatabaseRepository::new()),
        github_repo: Arc::new(MockGithubRepository::default()),
        volume_repo: Arc::new(mock_volume_repo),
        scheduler: Arc::new(mikrom_api::domain::MockScheduler::new()),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client),
        router_addr: "".to_string(),
        frontend_url: "".to_string(),
        jwt_secret: jwt_secret.into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        api_db: db.pool().clone(),
        acme_email: "".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        mesh_status: tokio::sync::watch::channel(
            mikrom_api::application::vms::MeshStatus::default(),
        )
        .0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/snapshots/{}", snapshot_id))
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_clone_volume_endpoint() {
    let _guard = volume_api_test_lock().lock().await;
    let mock_user_repo = MockUserRepository::new();
    let mock_app_repo = MockAppRepository::new();
    let mut mock_volume_repo = MockVolumeRepository::new();

    let user_id = Uuid::new_v4();
    let source_volume_id = Uuid::new_v4();
    let target_volume_id = Uuid::new_v4();
    let jwt_secret = "test-secret";

    let token = mikrom_api::auth::jwt::create_token(
        &user_id.to_string(),
        "test@example.com",
        &mikrom_api::domain::user::UserRole::User,
        jwt_secret,
    )
    .unwrap();

    mock_volume_repo
        .expect_get_volume()
        .with(eq(source_volume_id))
        .returning(move |id| {
            Ok(Some(Volume {
                id,
                user_id,
                name: "source-vol".to_string(),
                size_mib: 1024,
                pool_name: "test-pool".to_string(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            }))
        });

    mock_volume_repo
        .expect_create_volume()
        .returning(move |params| {
            Ok(Volume {
                id: target_volume_id,
                user_id: params.user_id,
                name: params.name,
                size_mib: params.size_mib,
                pool_name: params.pool_name,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
        });

    let db = TestDb::new().await;
    let Some(nats_client) = common::get_nats_client_or_skip().await else {
        return;
    };

    let mut subscriber = nats_client
        .subscribe("mikrom.scheduler.clone_volume")
        .await
        .unwrap();
    let nats_client_clone = nats_client.clone();
    tokio::spawn(async move {
        if let Some(msg) = subscriber.next().await {
            let resp = mikrom_proto::scheduler::CloneVolumeResponse {
                success: true,
                message: "".to_string(),
            };
            let mut buf = Vec::new();
            prost::Message::encode(&resp, &mut buf).unwrap();
            nats_client_clone
                .publish(msg.reply.unwrap(), buf.into())
                .await
                .unwrap();
        }
    });

    let state = AppState {
        ctx: mikrom_api::application::ApiContext::default(),
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        database_repo: Arc::new(mikrom_api::domain::MockDatabaseRepository::new()),
        github_repo: Arc::new(MockGithubRepository::default()),
        volume_repo: Arc::new(mock_volume_repo),
        scheduler: Arc::new(mikrom_api::domain::MockScheduler::new()),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client),
        router_addr: "".to_string(),
        frontend_url: "".to_string(),
        jwt_secret: jwt_secret.into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        api_db: db.pool().clone(),
        acme_email: "".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        mesh_status: tokio::sync::watch::channel(
            mikrom_api::application::vms::MeshStatus::default(),
        )
        .0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/volumes/{}/clone", source_volume_id))
                .header("Authorization", format!("Bearer {}", token))
                .header("Content-Type", "application/json")
                .body(Body::from(
                    r#"{"name": "cloned-vol", "snapshot_name": "manual-snap"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let volume: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(volume["id"], target_volume_id.to_string());
    assert_eq!(volume["name"], "cloned-vol");
}

#[tokio::test]
async fn test_restore_volume_snapshot_endpoint_failure() {
    let _guard = volume_api_test_lock().lock().await;
    let mock_user_repo = MockUserRepository::new();
    let mock_app_repo = MockAppRepository::new();
    let mut mock_volume_repo = MockVolumeRepository::new();

    let user_id = Uuid::new_v4();
    let volume_id = Uuid::new_v4();
    let jwt_secret = "test-secret";

    let token = mikrom_api::auth::jwt::create_token(
        &user_id.to_string(),
        "test@example.com",
        &mikrom_api::domain::user::UserRole::User,
        jwt_secret,
    )
    .unwrap();

    mock_volume_repo
        .expect_get_volume()
        .with(eq(volume_id))
        .returning(move |id| {
            Ok(Some(Volume {
                id,
                user_id,
                name: "test-vol".to_string(),
                size_mib: 1024,
                pool_name: "test-pool".to_string(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            }))
        });

    let db = TestDb::new().await;
    let nats = TypedNatsClient::new_custom(Arc::new(RestoreSnapshotNats {
        success: false,
        message: "Image is busy".to_string(),
        request_count: AtomicUsize::new(0),
    }));

    let state = AppState {
        ctx: mikrom_api::application::ApiContext::default(),
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        database_repo: Arc::new(mikrom_api::domain::MockDatabaseRepository::new()),
        github_repo: Arc::new(MockGithubRepository::default()),
        volume_repo: Arc::new(mock_volume_repo),
        scheduler: Arc::new(mikrom_api::domain::MockScheduler::new()),
        nats,
        router_addr: "".to_string(),
        frontend_url: "".to_string(),
        jwt_secret: jwt_secret.into(),
        master_key: "key".into(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        api_db: db.pool().clone(),
        acme_email: "".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        mesh_status: tokio::sync::watch::channel(
            mikrom_api::application::vms::MeshStatus::default(),
        )
        .0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/volumes/{}/restore", volume_id))
                .header("Authorization", format!("Bearer {}", token))
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"snapshot_name": "snap1"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(error["error"], "Scheduler error: Image is busy");
}
