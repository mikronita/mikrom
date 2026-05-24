mod common;
use axum::extract::Query;
use axum::extract::State;
use axum::http::StatusCode;
use mikrom_api::AppState;
use mikrom_api::domain::MockAppRepository;
use mikrom_api::domain::MockScheduler;
use mikrom_api::domain::github::MockGithubRepository;
use mikrom_api::domain::user::MockUserRepository;
use mikrom_api::infrastructure::http::handlers::github::{
    __github_callback_impl as github_callback, InstallCallbackQuery,
};
use mikrom_api::nats::TypedNatsClient;
use std::sync::Arc;
use tokio::sync::broadcast;

async fn create_test_state() -> Option<AppState> {
    let nats_client = common::get_nats_client_or_skip().await?;

    Some(AppState {
        ctx: mikrom_api::application::ApiContext::default(),
        user_repo: Arc::new(MockUserRepository::new()),
        app_repo: Arc::new(MockAppRepository::new()),
        volume_repo: Arc::new(mikrom_api::domain::MockVolumeRepository::new()),
        github_repo: Arc::new(MockGithubRepository::default()),
        scheduler: Arc::new(MockScheduler::new()),
        nats: TypedNatsClient::new(nats_client),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        jwt_secret: "test-secret".to_string(),
        master_key: "test-key".into(),
        deployment_events: broadcast::channel(1).0,
        api_db: sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/dummy")
            .unwrap(),
        acme_email: "test@example.com".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: Some("123".to_string()),
        github_private_key: Some("dummy-key".to_string()),
        github_app_slug: Some("test-app".to_string()),
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        mesh_status: tokio::sync::watch::channel(
            mikrom_api::application::vms::MeshStatus::default(),
        )
        .0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    })
}

#[tokio::test]
async fn test_github_callback_missing_state_redirects_to_settings() {
    let Some(state) = create_test_state().await else {
        return;
    };
    let query = InstallCallbackQuery {
        installation_id: 12345,
        setup_action: "install".to_string(),
        state: None,
    };

    let redirect = github_callback(State(state), Query(query)).await.unwrap();

    use axum::response::IntoResponse;
    let response = redirect.into_response();

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response.headers().get("location").unwrap(),
        "http://localhost:3000/settings"
    );
}

#[tokio::test]
async fn test_github_callback_invalid_state_returns_error() {
    let Some(state) = create_test_state().await else {
        return;
    };
    let query = InstallCallbackQuery {
        installation_id: 12345,
        setup_action: "install".to_string(),
        state: Some("invalid-token".to_string()),
    };

    let result = github_callback(State(state), Query(query)).await;

    assert!(result.is_err());
    // Should be a BadRequest
}
