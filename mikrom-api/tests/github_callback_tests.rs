use axum::extract::{Query, State};
use axum::response::IntoResponse;
use mikrom_api::AppState;
use mikrom_api::domain::github::MockGithubRepository;
use mikrom_api::domain::{
    MockAppRepository, MockDatabaseRepository, MockScheduler, MockTenantRepository,
    MockUserRepository, MockVolumeRepository,
};
use mikrom_api::infrastructure::http::handlers::github::{
    __github_callback_impl as github_callback, InstallCallbackQuery,
};
use std::sync::Arc;

#[allow(clippy::field_reassign_with_default)]
fn build_state() -> AppState {
    let mut state = AppState::default();
    state.github_app_id = Some("123".to_string());
    state.github_private_key = Some("dummy-key".to_string());
    state.frontend_url = "http://[::1]:5173".to_string();
    state.user_repo = Arc::new(MockUserRepository::new());
    state.ctx.user_repo = state.user_repo.clone();
    state.tenant_repo = Arc::new(MockTenantRepository::new());
    state.ctx.tenant_repo = state.tenant_repo.clone();
    state.app_repo = Arc::new(MockAppRepository::new());
    state.ctx.app_repo = state.app_repo.clone();
    state.database_repo = Arc::new(MockDatabaseRepository::new());
    state.ctx.database_repo = state.database_repo.clone();
    state.volume_repo = Arc::new(MockVolumeRepository::new());
    state.ctx.volume_repo = state.volume_repo.clone();
    state.scheduler = Arc::new(MockScheduler::new());
    state.ctx.scheduler = state.scheduler.clone();
    state.github_repo = Arc::new(MockGithubRepository::default());
    state.ctx.github_repo = state.github_repo.clone();
    state
}

#[tokio::test]
async fn github_callback_without_state_redirects_to_settings() {
    let state = build_state();
    let response = github_callback(
        State(state),
        Query(InstallCallbackQuery {
            installation_id: 12345,
            setup_action: "install".to_string(),
            state: None,
        }),
    )
    .await
    .unwrap()
    .into_response();

    assert_eq!(response.status(), axum::http::StatusCode::SEE_OTHER);
    assert_eq!(
        response.headers().get("location").unwrap(),
        "http://localhost:5173/settings"
    );
}

#[tokio::test]
async fn github_callback_with_invalid_state_errors() {
    let state = build_state();
    let result = github_callback(
        State(state),
        Query(InstallCallbackQuery {
            installation_id: 12345,
            setup_action: "install".to_string(),
            state: Some("invalid-token".to_string()),
        }),
    )
    .await;

    let err = result.expect_err("invalid state should fail");
    assert_eq!(
        err.to_string(),
        "Bad request: Invalid or expired state parameter"
    );
}

#[tokio::test]
async fn github_callback_requires_github_credentials() {
    let mut state = build_state();
    state.github_app_id = None;

    let result = github_callback(
        State(state),
        Query(InstallCallbackQuery {
            installation_id: 12345,
            setup_action: "install".to_string(),
            state: None,
        }),
    )
    .await;

    let err = result.expect_err("missing app id should fail");
    assert_eq!(
        err.to_string(),
        "Internal server error: GITHUB_APP_ID not configured"
    );
}

#[tokio::test]
async fn github_callback_requires_private_key() {
    let mut state = build_state();
    state.github_private_key = None;

    let result = github_callback(
        State(state),
        Query(InstallCallbackQuery {
            installation_id: 12345,
            setup_action: "install".to_string(),
            state: None,
        }),
    )
    .await;

    let err = result.expect_err("missing private key should fail");
    assert_eq!(
        err.to_string(),
        "Internal server error: GITHUB_PRIVATE_KEY not configured"
    );
}
