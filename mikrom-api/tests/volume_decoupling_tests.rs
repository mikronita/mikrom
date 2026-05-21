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
use mikrom_api::auth::jwt::create_token;
use mikrom_api::create_app;
use mikrom_api::models::volume::{AppVolume, Volume, VolumeAttachmentInfo, VolumeWithAttachments};
use mikrom_api::repositories::app_repository::MockAppRepository;
use mikrom_api::repositories::github_repository::MockGithubRepository;
use mikrom_api::repositories::user_repository::{MockUserRepository, UserRole};
use mikrom_api::repositories::volume_repository::MockVolumeRepository;

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

#[tokio::test]
async fn test_create_global_volume() {
    let mock_user_repo = MockUserRepository::new();
    let mock_app_repo = MockAppRepository::new();
    let mut mock_volume_repo = MockVolumeRepository::new();

    let user_id = Uuid::new_v4();
    let jwt_secret = "test-secret";
    let token = create_token(
        &user_id.to_string(),
        "test@example.com",
        &UserRole::User,
        jwt_secret,
    )
    .unwrap();

    mock_volume_repo
        .expect_create_volume()
        .returning(move |params| {
            Ok(Volume {
                id: Uuid::new_v4(),
                user_id: params.user_id,
                name: params.name,
                size_mib: params.size_mib,
                pool_name: params.pool_name,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
        });

    let db = TestDb::new().await;
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();
    let mut subscriber = nats_client
        .subscribe("mikrom.scheduler.create_volume")
        .await
        .unwrap();
    let nats_client_clone = nats_client.clone();
    tokio::spawn(async move {
        if let Some(msg) = subscriber.next().await {
            let resp = mikrom_proto::scheduler::CreateVolumeResponse {
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
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        github_repo: Arc::new(MockGithubRepository::default()),
        volume_repo: Arc::new(mock_volume_repo),
        scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
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
        mesh_status: tokio::sync::watch::channel(mikrom_api::vms::MeshStatus::default()).0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    let app = create_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/volumes")
                .header("Authorization", format!("Bearer {}", token))
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"name": "global-vol", "size_mib": 1024}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn test_attach_volume_to_app() {
    let mock_user_repo = MockUserRepository::new();
    let mut mock_app_repo = MockAppRepository::new();
    let mut mock_volume_repo = MockVolumeRepository::new();

    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let volume_id = Uuid::new_v4();
    let jwt_secret = "test-secret";
    let token = create_token(
        &user_id.to_string(),
        "test@example.com",
        &UserRole::User,
        jwt_secret,
    )
    .unwrap();

    mock_app_repo
        .expect_get_app()
        .with(eq(app_id))
        .returning(move |_| {
            let app = mikrom_api::models::app::App {
                id: app_id,
                user_id,
                ..mikrom_api::models::app::App::default()
            };
            Ok(Some(app))
        });

    mock_volume_repo
        .expect_get_volume()
        .with(eq(volume_id))
        .returning(move |_| {
            Ok(Some(Volume {
                id: volume_id,
                user_id,
                name: "test-vol".to_string(),
                size_mib: 1024,
                pool_name: "test-pool".to_string(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            }))
        });

    mock_volume_repo
        .expect_attach_volume_to_app()
        .with(eq(app_id), eq(volume_id), eq("/data".to_string()), eq(0))
        .returning(move |app_id, volume_id, mount_point, access_mode| {
            Ok(AppVolume {
                app_id,
                volume_id,
                mount_point,
                access_mode,
                created_at: Utc::now(),
            })
        });
    mock_volume_repo
        .expect_list_volumes_by_app()
        .with(eq(app_id))
        .returning(|_| Ok(vec![]));
    mock_volume_repo
        .expect_list_volumes_by_user()
        .with(eq(user_id))
        .returning(move |_| {
            Ok(vec![VolumeWithAttachments {
                volume: Volume {
                    id: volume_id,
                    user_id,
                    name: "test-vol".to_string(),
                    size_mib: 1024,
                    pool_name: "test-pool".to_string(),
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                },
                attachments: vec![],
            }])
        });

    let db = TestDb::new().await;
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();

    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        github_repo: Arc::new(MockGithubRepository::default()),
        volume_repo: Arc::new(mock_volume_repo),
        scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
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
        mesh_status: tokio::sync::watch::channel(mikrom_api::vms::MeshStatus::default()).0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    let app = create_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/apps/{}/volumes/attach", app_id))
                .header("Authorization", format!("Bearer {}", token))
                .header("Content-Type", "application/json")
                .body(Body::from(format!(
                    r#"{{"volume_id": "{}", "mount_point": "/data", "access_mode": 0}}"#,
                    volume_id
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_attach_volume_rejects_rwo_conflict() {
    let mock_user_repo = MockUserRepository::new();
    let mut mock_app_repo = MockAppRepository::new();
    let mut mock_volume_repo = MockVolumeRepository::new();

    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let other_app_id = Uuid::new_v4();
    let volume_id = Uuid::new_v4();
    let jwt_secret = "test-secret";
    let token = create_token(
        &user_id.to_string(),
        "test@example.com",
        &UserRole::User,
        jwt_secret,
    )
    .unwrap();

    mock_app_repo
        .expect_get_app()
        .with(eq(app_id))
        .returning(move |_| {
            let app = mikrom_api::models::app::App {
                id: app_id,
                user_id,
                ..mikrom_api::models::app::App::default()
            };
            Ok(Some(app))
        });

    mock_volume_repo
        .expect_get_volume()
        .with(eq(volume_id))
        .returning(move |_| {
            Ok(Some(Volume {
                id: volume_id,
                user_id,
                name: "test-vol".to_string(),
                size_mib: 1024,
                pool_name: "test-pool".to_string(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            }))
        });

    mock_volume_repo
        .expect_list_volumes_by_app()
        .with(eq(app_id))
        .returning(|_| Ok(vec![]));

    mock_volume_repo
        .expect_list_volumes_by_user()
        .with(eq(user_id))
        .returning(move |_| {
            Ok(vec![VolumeWithAttachments {
                volume: Volume {
                    id: volume_id,
                    user_id,
                    name: "test-vol".to_string(),
                    size_mib: 1024,
                    pool_name: "test-pool".to_string(),
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                },
                attachments: vec![VolumeAttachmentInfo {
                    app_id: other_app_id,
                    app_name: "other-app".to_string(),
                    mount_point: "/data".to_string(),
                    access_mode: 0,
                }],
            }])
        });

    let db = TestDb::new().await;
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();

    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        github_repo: Arc::new(MockGithubRepository::default()),
        volume_repo: Arc::new(mock_volume_repo),
        scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
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
        mesh_status: tokio::sync::watch::channel(mikrom_api::vms::MeshStatus::default()).0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    let app = create_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/apps/{}/volumes/attach", app_id))
                .header("Authorization", format!("Bearer {}", token))
                .header("Content-Type", "application/json")
                .body(Body::from(format!(
                    r#"{{"volume_id": "{}", "mount_point": "/data", "access_mode": 0}}"#,
                    volume_id
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_list_volumes_with_attachments() {
    let mock_user_repo = MockUserRepository::new();
    let mock_app_repo = MockAppRepository::new();
    let mut mock_volume_repo = MockVolumeRepository::new();

    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let volume_id = Uuid::new_v4();
    let jwt_secret = "test-secret";
    let token = create_token(
        &user_id.to_string(),
        "test@example.com",
        &UserRole::User,
        jwt_secret,
    )
    .unwrap();

    mock_volume_repo
        .expect_list_volumes_by_user()
        .with(eq(user_id))
        .returning(move |_| {
            Ok(vec![VolumeWithAttachments {
                volume: Volume {
                    id: volume_id,
                    user_id,
                    name: "test-vol".to_string(),
                    size_mib: 1024,
                    pool_name: "test-pool".to_string(),
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                },
                attachments: vec![VolumeAttachmentInfo {
                    app_id,
                    app_name: "test-app".to_string(),
                    mount_point: "/data".to_string(),
                    access_mode: 0,
                }],
            }])
        });

    let db = TestDb::new().await;
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();

    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        github_repo: Arc::new(MockGithubRepository::default()),
        volume_repo: Arc::new(mock_volume_repo),
        scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
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
        mesh_status: tokio::sync::watch::channel(mikrom_api::vms::MeshStatus::default()).0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    let app = create_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/volumes")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_delete_volume_fails_if_attached() {
    let mock_user_repo = MockUserRepository::new();
    let mock_app_repo = MockAppRepository::new();
    let mut mock_volume_repo = MockVolumeRepository::new();

    let user_id = Uuid::new_v4();
    let volume_id = Uuid::new_v4();
    let jwt_secret = "test-secret";
    let token = create_token(
        &user_id.to_string(),
        "test@example.com",
        &UserRole::User,
        jwt_secret,
    )
    .unwrap();

    mock_volume_repo
        .expect_get_volume()
        .with(eq(volume_id))
        .returning(move |_| {
            Ok(Some(Volume {
                id: volume_id,
                user_id,
                name: "test-vol".to_string(),
                size_mib: 1024,
                pool_name: "test-pool".to_string(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            }))
        });

    mock_volume_repo
        .expect_is_volume_attached()
        .with(eq(volume_id))
        .returning(|_| Ok(true));
    mock_volume_repo
        .expect_list_snapshots_by_volume()
        .with(eq(volume_id))
        .returning(|_| Ok(vec![]));

    let db = TestDb::new().await;
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.unwrap();

    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        github_repo: Arc::new(MockGithubRepository::default()),
        volume_repo: Arc::new(mock_volume_repo),
        scheduler: Arc::new(mikrom_api::scheduler::MockScheduler::new()),
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
        mesh_status: tokio::sync::watch::channel(mikrom_api::vms::MeshStatus::default()).0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    let app = create_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/volumes/{}", volume_id))
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
