use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mikrom_api::AppState;
use mikrom_api::auth::jwt::create_token;
use mikrom_api::create_app;
use mikrom_api::domain::app::App;
use mikrom_api::domain::user::{MockUserRepository, UserRole};
use mikrom_api::domain::{
    MockAppRepository, MockDatabaseRepository, MockScheduler, MockTenantRepository,
    MockVolumeRepository,
};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

#[allow(clippy::field_reassign_with_default)]
fn build_state(app: App) -> AppState {
    let mut user_repo = MockUserRepository::new();
    user_repo.expect_find_by_email().returning(|_| Ok(None));

    let mut app_repo = MockAppRepository::new();
    app_repo
        .expect_get_app_by_name()
        .returning(move |_| Ok(Some(app.clone())));

    let mut state = AppState::default();
    state.jwt_secret = "test-secret".to_string();
    state.ctx.jwt_secret = state.jwt_secret.clone();
    state.user_repo = Arc::new(user_repo);
    state.ctx.user_repo = state.user_repo.clone();
    state.app_repo = Arc::new(app_repo);
    state.ctx.app_repo = state.app_repo.clone();
    state.database_repo = Arc::new(MockDatabaseRepository::new());
    state.ctx.database_repo = state.database_repo.clone();
    state.volume_repo = Arc::new(MockVolumeRepository::new());
    state.ctx.volume_repo = state.volume_repo.clone();
    state.tenant_repo = Arc::new(MockTenantRepository::new());
    state.ctx.tenant_repo = state.tenant_repo.clone();
    let mut scheduler = MockScheduler::new();
    scheduler
        .expect_list_apps()
        .returning(|_| Ok(mikrom_proto::scheduler::ListAppsResponse { apps: vec![] }));
    state.scheduler = Arc::new(scheduler);
    state.ctx.scheduler = state.scheduler.clone();
    state
}

#[tokio::test]
#[ignore = "requires a stable NATS subscriber fixture"]
async fn logs_stream_route_is_exposed() {
    let user_id = Uuid::new_v4();
    let app = App {
        id: Uuid::new_v4(),
        name: "observed-app".to_string(),
        git_url: "https://github.com/test/repo".to_string(),
        port: mikrom_api::domain::types::Port::new(8080).unwrap(),
        tenant_id: user_id,
        ..App::default()
    };
    let token = create_token(
        &user_id.to_string(),
        "test@example.com",
        &UserRole::User,
        "test-secret",
    )
    .unwrap();

    let app_router = create_app(build_state(app));
    let response = app_router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/apps/observed-app/logs/stream")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/event-stream"
    );
}

#[tokio::test]
async fn logs_stream_route_rejects_foreign_tenant() {
    let app_tenant_id = Uuid::new_v4();
    let foreign_tenant_id = Uuid::new_v4();
    let app = App {
        id: Uuid::new_v4(),
        name: "observed-app".to_string(),
        git_url: "https://github.com/test/repo".to_string(),
        port: mikrom_api::domain::types::Port::new(8080).unwrap(),
        tenant_id: app_tenant_id,
        ..App::default()
    };
    let token = create_token(
        &foreign_tenant_id.to_string(),
        "test@example.com",
        &UserRole::User,
        "test-secret",
    )
    .unwrap();

    let app_router = create_app(build_state(app));
    let response = app_router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/apps/observed-app/logs/stream")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn logs_stream_route_requires_authentication() {
    let app = create_app(build_state(App {
        id: Uuid::new_v4(),
        name: "observed-app".to_string(),
        git_url: "https://github.com/test/repo".to_string(),
        port: mikrom_api::domain::types::Port::new(8080).unwrap(),
        tenant_id: Uuid::new_v4(),
        ..App::default()
    }));

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/apps/observed-app/logs/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[ignore = "requires a stable NATS subscriber fixture"]
async fn metrics_stream_route_is_exposed() {
    let user_id = Uuid::new_v4();
    let app = App {
        id: Uuid::new_v4(),
        name: "observed-app".to_string(),
        git_url: "https://github.com/test/repo".to_string(),
        port: mikrom_api::domain::types::Port::new(8080).unwrap(),
        tenant_id: user_id,
        ..App::default()
    };
    let token = create_token(
        &user_id.to_string(),
        "test@example.com",
        &UserRole::User,
        "test-secret",
    )
    .unwrap();

    let app_router = create_app(build_state(app));
    let response = app_router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/apps/observed-app/metrics/stream")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/event-stream"
    );
}

#[tokio::test]
async fn metrics_stream_route_rejects_foreign_tenant() {
    let app_tenant_id = Uuid::new_v4();
    let foreign_tenant_id = Uuid::new_v4();
    let app = App {
        id: Uuid::new_v4(),
        name: "observed-app".to_string(),
        git_url: "https://github.com/test/repo".to_string(),
        port: mikrom_api::domain::types::Port::new(8080).unwrap(),
        tenant_id: app_tenant_id,
        ..App::default()
    };
    let token = create_token(
        &foreign_tenant_id.to_string(),
        "test@example.com",
        &UserRole::User,
        "test-secret",
    )
    .unwrap();

    let app_router = create_app(build_state(app));
    let response = app_router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/apps/observed-app/metrics/stream")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn metrics_stream_route_requires_authentication() {
    let app = create_app(build_state(App {
        id: Uuid::new_v4(),
        name: "observed-app".to_string(),
        git_url: "https://github.com/test/repo".to_string(),
        port: mikrom_api::domain::types::Port::new(8080).unwrap(),
        tenant_id: Uuid::new_v4(),
        ..App::default()
    }));

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/apps/observed-app/metrics/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
