use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mikrom_api::domain::{Database, DatabaseStatus, MockUserRepository};
use mikrom_api::infrastructure::db::PostgresAppRepository;
use mikrom_api::nats::NatsClient;
use mikrom_api::test_utils::create_test_app_state;
use mikrom_api::{AppState, create_app};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

struct DummyNats;

#[async_trait::async_trait]
impl NatsClient for DummyNats {
    async fn request_raw(&self, _subject: String, _payload: Vec<u8>) -> anyhow::Result<Vec<u8>> {
        Err(anyhow::anyhow!("unexpected NATS request"))
    }

    async fn publish_raw(&self, _subject: String, _payload: Vec<u8>) -> anyhow::Result<()> {
        Ok(())
    }

    async fn subscribe_raw(&self, _subject: String) -> anyhow::Result<async_nats::Subscriber> {
        Err(anyhow::anyhow!("unexpected NATS subscribe"))
    }
}

async fn build_state() -> AppState {
    let db = sqlx::PgPool::connect_lazy("postgres://localhost/dummy").unwrap();
    let mut state = create_test_app_state(db.clone());
    let app_repo = Arc::new(PostgresAppRepository::new(db, "key".to_string()));
    state.app_repo = app_repo.clone();
    state.ctx.app_repo = app_repo;

    let tenant_id = "11111111111111111111111111111111".to_string();
    let mut database_repo = mikrom_api::domain::MockDatabaseRepository::new();
    let tenant_id_for_return = tenant_id.clone();
    database_repo
        .expect_list_databases()
        .times(0..=1)
        .returning(move || {
            let db = Database {
                id: Uuid::new_v4(),
                name: "orders".to_string(),
                engine: "neon".to_string(),
                user_id: Uuid::new_v4(),
                vcpus: mikrom_api::domain::types::CpuCores::try_from(1).unwrap(),
                memory_mib: mikrom_api::domain::types::MemoryMb::try_from(512).unwrap(),
                disk_mib: 1024,
                tenant_id: Some(tenant_id_for_return.clone()),
                timeline_id: Some("22222222222222222222222222222222".to_string()),
                tenant_gen: Some(1),
                settings: HashMap::new(),
                status: DatabaseStatus::Running,
                active_deployment_id: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            };
            Box::pin(async move { Ok(vec![db]) })
        });
    database_repo
        .expect_get_database_by_tenant_id()
        .with(mockall::predicate::eq(tenant_id.clone()))
        .times(0..=1)
        .returning(move |_| {
            let db = Database {
                id: Uuid::new_v4(),
                name: "orders".to_string(),
                engine: "neon".to_string(),
                user_id: Uuid::new_v4(),
                vcpus: mikrom_api::domain::types::CpuCores::try_from(1).unwrap(),
                memory_mib: mikrom_api::domain::types::MemoryMb::try_from(512).unwrap(),
                disk_mib: 1024,
                tenant_id: Some(tenant_id.clone()),
                timeline_id: Some("22222222222222222222222222222222".to_string()),
                tenant_gen: Some(1),
                settings: HashMap::new(),
                status: DatabaseStatus::Running,
                active_deployment_id: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            };
            Box::pin(async move { Ok(Some(db)) })
        });
    let database_repo = Arc::new(database_repo);
    state.database_repo = database_repo.clone();
    state.ctx.database_repo = database_repo;

    state.nats = mikrom_api::nats::TypedNatsClient::new_custom(Arc::new(DummyNats));
    state.ctx.nats = state.nats.clone();
    state.jwt_secret = "test".to_string();
    state.ctx.jwt_secret = "test".to_string();
    state.master_key = "test".to_string();
    state.ctx.master_key = "test".to_string();
    state
}

async fn build_state_with_databases(databases: Vec<Database>) -> AppState {
    let db = sqlx::PgPool::connect_lazy("postgres://localhost/dummy").unwrap();
    let mut state = create_test_app_state(db.clone());
    let app_repo = Arc::new(PostgresAppRepository::new(db, "key".to_string()));
    state.app_repo = app_repo.clone();
    state.ctx.app_repo = app_repo;

    let mut database_repo = mikrom_api::domain::MockDatabaseRepository::new();
    database_repo
        .expect_list_databases()
        .times(0..=1)
        .returning(move || {
            let value = databases.clone();
            Box::pin(async move { Ok(value) })
        });
    let database_repo = Arc::new(database_repo);
    state.database_repo = database_repo.clone();
    state.ctx.database_repo = database_repo;

    state.nats = mikrom_api::nats::TypedNatsClient::new_custom(Arc::new(DummyNats));
    state.ctx.nats = state.nats.clone();
    state.jwt_secret = "test".to_string();
    state.ctx.jwt_secret = "test".to_string();
    state.master_key = "test".to_string();
    state.ctx.master_key = "test".to_string();
    state
}

fn database_with_status(
    tenant_id: Option<&str>,
    timeline_id: Option<&str>,
    tenant_gen: Option<u32>,
    engine: &str,
    status: DatabaseStatus,
) -> Database {
    Database {
        id: Uuid::new_v4(),
        name: "orders".to_string(),
        engine: engine.to_string(),
        user_id: Uuid::new_v4(),
        vcpus: mikrom_api::domain::types::CpuCores::try_from(1).unwrap(),
        memory_mib: mikrom_api::domain::types::MemoryMb::try_from(512).unwrap(),
        disk_mib: 1024,
        tenant_id: tenant_id.map(|v| v.to_string()),
        timeline_id: timeline_id.map(|v| v.to_string()),
        tenant_gen,
        settings: HashMap::new(),
        status,
        active_deployment_id: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

#[tokio::test]
async fn test_v1_health_routing() {
    let app = create_app(build_state().await);

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // 404 would mean prefix is missing or wrong
    // It might be 200 or 500 (if health check fails due to DB/NATS), but not 404
    assert_ne!(
        response.status(),
        StatusCode::NOT_FOUND,
        "GET /v1/health should not be 404"
    );
}

#[tokio::test]
async fn test_v1_auth_routing() {
    let state = build_state().await;
    let mut user_repo = MockUserRepository::new();
    user_repo.expect_find_by_email().returning(|_| Ok(None)); // Just return None to avoid 404/panic
    let mut state = state;
    state.user_repo = Arc::new(user_repo);
    state.ctx.user_repo = state.user_repo.clone();
    let app = create_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"email":"test@example.com","password":"password"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_ne!(
        response.status(),
        StatusCode::NOT_FOUND,
        "POST /v1/auth/login should not be 404"
    );
}

#[tokio::test]
async fn test_v1_apps_routing() {
    let app = create_app(build_state().await);

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/apps")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // It will likely be 401 Unauthorized because we don't provide a JWT, but not 404
    assert_ne!(
        response.status(),
        StatusCode::NOT_FOUND,
        "GET /v1/apps should not be 404"
    );
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_v1_database_routes_are_registered() {
    let app = create_app(build_state().await);

    for (method, uri) in [
        ("GET", "/v1/databases"),
        ("POST", "/v1/databases"),
        (
            "DELETE",
            "/v1/databases/00000000-0000-0000-0000-000000000000",
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_ne!(
            response.status(),
            StatusCode::NOT_FOUND,
            "{method} {uri} should not be 404"
        );
    }
}

#[tokio::test]
async fn test_re_attach_root_route_is_registered() {
    let app = create_app(build_state().await);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/re-attach")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "tenants": [
                {
                    "id": "11111111111111111111111111111111",
                    "gen": 1
                }
            ]
        })
    );
}

#[tokio::test]
async fn test_re_attach_filters_out_inactive_and_placeholder_databases() {
    let active = database_with_status(
        Some("11111111111111111111111111111111"),
        Some("22222222222222222222222222222222"),
        Some(7),
        "neon",
        DatabaseStatus::Running,
    );
    let deleted = database_with_status(
        Some("33333333333333333333333333333333"),
        Some("44444444444444444444444444444444"),
        Some(1),
        "neon",
        DatabaseStatus::Deleting,
    );
    let failed = database_with_status(
        Some("55555555555555555555555555555555"),
        Some("66666666666666666666666666666666"),
        Some(1),
        "neon",
        DatabaseStatus::Failed,
    );
    let non_neon = database_with_status(
        Some("77777777777777777777777777777777"),
        Some("88888888888888888888888888888888"),
        Some(1),
        "postgres",
        DatabaseStatus::Running,
    );
    let placeholder = database_with_status(
        Some("pending-tenant-123"),
        Some("pending-timeline-456"),
        Some(1),
        "neon",
        DatabaseStatus::Pending,
    );

    let app = create_app(
        build_state_with_databases(vec![active, deleted, failed, non_neon, placeholder]).await,
    );

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/re-attach")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "tenants": [
                {
                    "id": "11111111111111111111111111111111",
                    "gen": 7
                }
            ]
        })
    );
}

#[tokio::test]
async fn test_validate_root_route_is_registered() {
    let app = create_app(build_state().await);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/validate")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"tenants":[{"id":"11111111111111111111111111111111","gen":1}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "tenants": [
                {
                    "id": "11111111111111111111111111111111",
                    "valid": true
                }
            ]
        })
    );
}
