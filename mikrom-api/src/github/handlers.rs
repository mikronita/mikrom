use crate::AppState;
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::github::{GithubRepo, list_installation_repos};
use crate::models::github::UserGithubAccount;
use axum::Json;
use axum::extract::{Query, State};
use axum::response::Redirect;
use serde::Deserialize;
#[derive(Debug, Deserialize)]
pub struct InstallCallbackQuery {
    pub installation_id: i64,
    pub setup_action: String,
    pub state: Option<String>,
}

#[utoipa::path(
    get,
    path = "/github/install",
    responses(
        (status = 200, description = "Get GitHub App installation URL", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "auth",
    security(
        ("jwt" = [])
    )
)]
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
        &crate::repositories::user_repository::UserRole::User,
        &state.jwt_secret,
    )
    .map_err(|e| ApiError::Internal(format!("Failed to create state token: {}", e)))?;

    Ok(Json(serde_json::json!({
        "url": format!("https://github.com/apps/{}/installations/new?state={}", slug, state_token)
    })))
}

#[utoipa::path(
    get,
    path = "/github/callback",
    params(
        ("installation_id" = i64, Query, description = "GitHub installation ID"),
        ("setup_action" = String, Query, description = "Setup action"),
        ("state" = Option<String>, Query, description = "State token for CSRF protection")
    ),
    responses(
        (status = 302, description = "Redirect back to frontend after successful installation"),
        (status = 400, description = "Invalid callback parameters", body = crate::error::ErrorResponse)
    ),
    tag = "auth"
)]
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

    let state_token = query
        .state
        .ok_or_else(|| ApiError::BadRequest("Missing state parameter from GitHub".to_string()))?;

    let claims = crate::auth::jwt::verify_token(&state_token, &state.jwt_secret)
        .map_err(|_| ApiError::BadRequest("Invalid or expired state parameter".to_string()))?;

    let user_id = uuid::Uuid::parse_str(&claims.sub)
        .map_err(|_| ApiError::BadRequest("Invalid user ID in state".to_string()))?;

    // Verify installation and get username
    let jwt = crate::github::generate_jwt(app_id, private_key)?;

    let response = crate::github::HTTP_CLIENT
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

    // Redirect back to settings in the frontend
    Ok(Redirect::to(&format!("{}/settings", state.frontend_url)))
}

#[utoipa::path(
    get,
    path = "/github/repos",
    responses(
        (status = 200, description = "List available GitHub repositories", body = [GithubRepo]),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "auth",
    security(
        ("jwt" = [])
    )
)]
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

#[utoipa::path(
    get,
    path = "/github/accounts",
    responses(
        (status = 200, description = "List connected GitHub accounts", body = [UserGithubAccount]),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "auth",
    security(
        ("jwt" = [])
    )
)]
pub async fn list_accounts(
    State(state): State<AppState>,
    auth: AuthUser,
) -> ApiResult<Json<Vec<UserGithubAccount>>> {
    let user_id =
        uuid::Uuid::parse_str(&auth.user_id).map_err(|e| ApiError::Internal(e.to_string()))?;
    let accounts = state.github_repo.get_accounts_by_user_id(user_id).await?;
    Ok(Json(accounts))
}
