use crate::AppState;
use crate::auth::AuthUser;
use crate::domain::UserGithubAccount;
use crate::error::{ApiError, ApiResult};
use crate::infrastructure::github::{GithubRepo, list_installation_repos};
use crate::normalize_service_url;
use crate::workspace::{WorkspaceEvent, WorkspaceEventKind};
use axum::Json;
use axum::extract::{Query, State};
use axum::response::Redirect;
use serde::Deserialize;
#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct InstallCallbackQuery {
    pub installation_id: i64,
    pub setup_action: String,
    pub state: Option<String>,
}

#[rovo::rovo]
pub async fn github_install(
    auth: AuthUser,
    State(state): State<AppState>,
) -> ApiResult<Json<serde_json::Value>> {
    let slug = state
        .github_app_slug
        .ok_or_else(|| ApiError::Internal("GITHUB_APP_SLUG not configured".to_string()))?;

    // Create a short-lived state token for CSRF protection
    let state_token = crate::auth::jwt::create_token(
        &auth.user_id,
        &auth.email,
        &crate::domain::UserRole::User,
        &state.jwt_secret,
    )
    .map_err(|e| ApiError::Internal(format!("Failed to create state token: {}", e)))?;

    Ok(Json(serde_json::json!({
        "url": format!("https://github.com/apps/{}/installations/new?state={}", slug, state_token)
    })))
}

#[rovo::rovo(skip)]
pub async fn github_callback(
    State(state): State<AppState>,
    Query(query): Query<InstallCallbackQuery>,
) -> ApiResult<Redirect> {
    let app_id = state
        .github_app_id
        .as_ref()
        .ok_or_else(|| ApiError::Internal("GITHUB_APP_ID not configured".to_string()))?;
    let private_key = state
        .github_private_key
        .as_ref()
        .ok_or_else(|| ApiError::Internal("GITHUB_PRIVATE_KEY not configured".to_string()))?;

    let state_token = match query.state {
        Some(token) => token,
        None => {
            tracing::info!(
                "GitHub callback without state parameter. installation_id: {}, setup_action: {}",
                query.installation_id,
                query.setup_action
            );
            return Ok(Redirect::to(&format!(
                "{}/settings",
                normalize_service_url(&state.frontend_url)
            )));
        },
    };

    let claims = crate::auth::jwt::verify_token(&state_token, &state.jwt_secret)
        .map_err(|_| ApiError::BadRequest("Invalid or expired state parameter".to_string()))?;

    let user_id = uuid::Uuid::parse_str(&claims.sub)
        .map_err(|_| ApiError::BadRequest("Invalid user ID in state".to_string()))?;

    // Verify installation and get username
    let jwt = crate::infrastructure::github::generate_jwt(app_id, private_key)?;

    let response = crate::infrastructure::github::HTTP_CLIENT
        .get(format!(
            "https://api.github.com/app/installations/{}",
            query.installation_id
        ))
        .header("Authorization", format!("Bearer {}", jwt))
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| ApiError::Internal(format!("GitHub API request failed: {}", e)))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "Could not read body".to_string());
        tracing::error!(
            "Failed to verify GitHub installation. Status: {}, Body: {}",
            status,
            body
        );
        return Err(ApiError::Internal(format!(
            "Failed to verify GitHub installation: {}",
            status
        )));
    }

    #[derive(Deserialize)]
    struct InstallationResponse {
        account: Account,
    }
    #[derive(Deserialize)]
    struct Account {
        login: String,
    }

    let install_data: InstallationResponse = response
        .json()
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to parse installation data: {}", e)))?;

    let account = UserGithubAccount {
        id: uuid::Uuid::new_v4(),
        user_id,
        installation_id: query.installation_id,
        github_username: install_data.account.login,
        created_at: chrono::Utc::now(),
    };

    state.github_repo.create_account(account).await?;

    state.publish_workspace_event(WorkspaceEvent {
        kind: WorkspaceEventKind::GithubAccountsChanged,
        user_id: Some(user_id),
        tenant_id: None,
        app_id: None,
        app_name: None,
        deployment_id: None,
        volume_id: None,
        resource_id: Some(query.installation_id.to_string()),
    });

    // Redirect back to settings in the frontend
    Ok(Redirect::to(&format!(
        "{}/settings",
        normalize_service_url(&state.frontend_url)
    )))
}

#[rovo::rovo]
pub async fn list_repos(
    State(state): State<AppState>,
    auth: AuthUser,
) -> ApiResult<Json<Vec<GithubRepo>>> {
    let app_id = state
        .github_app_id
        .clone()
        .ok_or_else(|| ApiError::Internal("GITHUB_APP_ID not configured".to_string()))?;
    let private_key = state
        .github_private_key
        .clone()
        .ok_or_else(|| ApiError::Internal("GITHUB_PRIVATE_KEY not configured".to_string()))?;

    let user_id =
        uuid::Uuid::parse_str(&auth.user_id).map_err(|e| ApiError::Internal(e.to_string()))?;

    let accounts = state.github_repo.get_accounts_by_user_id(user_id).await?;

    let futures = accounts.into_iter().map(|account| {
        let app_id = app_id.clone();
        let private_key = private_key.clone();
        async move { list_installation_repos(&app_id, &private_key, account.installation_id).await }
    });

    let results = futures::future::join_all(futures).await;
    let mut all_repos = Vec::new();

    for res in results {
        match res {
            Ok(repos) => all_repos.extend(repos),
            Err(e) => tracing::error!("Failed to list repos: {}", e),
        }
    }

    Ok(Json(all_repos))
}

#[rovo::rovo]
pub async fn list_accounts(
    State(state): State<AppState>,
    auth: AuthUser,
) -> ApiResult<Json<Vec<UserGithubAccount>>> {
    let user_id =
        uuid::Uuid::parse_str(&auth.user_id).map_err(|e| ApiError::Internal(e.to_string()))?;
    let accounts = state.github_repo.get_accounts_by_user_id(user_id).await?;
    Ok(Json(accounts))
}
