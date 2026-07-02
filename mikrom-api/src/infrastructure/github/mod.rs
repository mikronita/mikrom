use crate::error::{ApiError, ApiResult};
use chrono::{Duration, Utc};
use jsonwebtoken::{EncodingKey, Header};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    iat: i64,
    exp: i64,
    iss: String,
}

#[derive(Debug, Deserialize)]
pub struct InstallationTokenResponse {
    pub token: String,
    pub expires_at: String,
}

#[derive(Debug, Serialize, Deserialize, rovo::schemars::JsonSchema)]
pub struct GithubRepo {
    pub id: i64,
    pub name: String,
    pub full_name: String,
    pub private: bool,
    pub html_url: String,
    pub description: Option<String>,
    pub installation_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct GithubReposResponse {
    repositories: Vec<GithubRepo>,
}

use std::sync::LazyLock;

pub(crate) static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent("mikrom-api")
        .build()
        .expect("Failed to create reqwest client")
});

pub fn generate_jwt(app_id: &str, private_key_pem: &str) -> ApiResult<String> {
    let iat = Utc::now().timestamp() - 60; // 60 seconds leeway
    let exp = (Utc::now() + Duration::minutes(10)).timestamp();

    let claims = Claims {
        iat,
        exp,
        iss: app_id.to_string(),
    };

    // Handle escaped newlines in private key if they exist
    let pem = private_key_pem
        .trim()
        .trim_matches('"')
        .replace("\\n", "\n");

    let key = EncodingKey::from_rsa_pem(pem.as_bytes())
        .map_err(|e| ApiError::Internal(format!("Invalid private key: {}", e)))?;

    jsonwebtoken::encode(&Header::new(jsonwebtoken::Algorithm::RS256), &claims, &key)
        .map_err(|e| ApiError::Internal(format!("Failed to encode JWT: {}", e)))
}

async fn get_installation_token_with_client(
    client: &reqwest::Client,
    api_base_url: &str,
    app_id: &str,
    private_key_pem: &str,
    installation_id: i64,
) -> ApiResult<String> {
    let jwt = generate_jwt(app_id, private_key_pem)?;

    let response = client
        .post(format!("{}/app/installations/{}/access_tokens", api_base_url, installation_id))
        .header("Authorization", format!("Bearer {}", jwt))
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "mikrom-api")
        .send()
        .await
        .map_err(|e| ApiError::Internal(format!("GitHub API request failed: {}", e)))?;

    if !response.status().is_success() {
        let status = response.status();
        let error_body = response.text().await.unwrap_or_default();
        return Err(ApiError::Internal(format!(
            "Failed to get installation token: {} - {}",
            status, error_body
        )));
    }

    let token_resp: InstallationTokenResponse = response
        .json()
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to parse token response: {}", e)))?;

    Ok(token_resp.token)
}

pub async fn get_installation_token(
    app_id: &str,
    private_key_pem: &str,
    installation_id: i64,
) -> ApiResult<String> {
    get_installation_token_with_client(
        &HTTP_CLIENT,
        "https://api.github.com",
        app_id,
        private_key_pem,
        installation_id,
    )
    .await
}

async fn list_installation_repos_with_client(
    client: &reqwest::Client,
    api_base_url: &str,
    app_id: &str,
    private_key_pem: &str,
    installation_id: i64,
) -> ApiResult<Vec<GithubRepo>> {
    let token = get_installation_token_with_client(client, api_base_url, app_id, private_key_pem, installation_id).await?;
    let mut all_repos = Vec::new();
    let mut page = 1;

    loop {
        let url = format!(
            "{}/installation/repositories?per_page=100&page={}",
            api_base_url, page
        );
        let response = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", "mikrom-api")
            .send()
            .await
            .map_err(|e| ApiError::Internal(format!("GitHub API request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            return Err(ApiError::Internal(format!(
                "Failed to list repositories: {} - {}",
                status, error_body
            )));
        }

        let mut repos_resp: GithubReposResponse = response
            .json()
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to parse repos response: {}", e)))?;

        if repos_resp.repositories.is_empty() {
            break;
        }

        for repo in &mut repos_resp.repositories {
            repo.installation_id = Some(installation_id);
        }

        let fetched_count = repos_resp.repositories.len();
        all_repos.extend(repos_resp.repositories);

        if fetched_count < 100 {
            break;
        }

        page += 1;
        if page > 100 {
            // Limit to 10,000 repos for safety
            break;
        }
    }

    Ok(all_repos)
}

pub async fn list_installation_repos(
    app_id: &str,
    private_key_pem: &str,
    installation_id: i64,
) -> ApiResult<Vec<GithubRepo>> {
    list_installation_repos_with_client(
        &HTTP_CLIENT,
        "https://api.github.com",
        app_id,
        private_key_pem,
        installation_id,
    )
    .await
}

async fn create_repository_webhook_with_client(
    client: &reqwest::Client,
    api_base_url: &str,
    app_id: &str,
    private_key_pem: &str,
    installation_id: i64,
    repo_full_name: &str,
    webhook_url: &str,
    webhook_secret: &str,
) -> ApiResult<()> {
    tracing::info!(repo = %repo_full_name, url = %webhook_url, "Creating GitHub repository webhook...");
    let token = get_installation_token_with_client(client, api_base_url, app_id, private_key_pem, installation_id).await?;

    let response = client
        .post(format!("{}/repos/{}/hooks", api_base_url, repo_full_name))
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "mikrom-api")
        .timeout(std::time::Duration::from_secs(10))
        .json(&serde_json::json!({
            "name": "web",
            "active": true,
            "events": ["push"],
            "config": {
                "url": webhook_url,
                "content_type": "json",
                "secret": webhook_secret,
                "insecure_ssl": "0"
            }
        }))
        .send()
        .await
        .map_err(|e| ApiError::Internal(format!("GitHub API request failed: {}", e)))?;

    if !response.status().is_success() {
        let status = response.status();
        let error_body = response.text().await.unwrap_or_default();
        if webhook_create_status_is_idempotent(status) {
            tracing::info!(repo = %repo_full_name, "GitHub webhook creation returned 422, treating as idempotent");
            return Ok(());
        }
        tracing::error!(repo = %repo_full_name, status = %status, body = %error_body, "Failed to create GitHub webhook");
        return Err(ApiError::Internal(format!(
            "Failed to create webhook: {} - {}",
            status, error_body
        )));
    }

    tracing::info!(repo = %repo_full_name, "Successfully created GitHub repository webhook");
    Ok(())
}

fn webhook_create_status_is_idempotent(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::UNPROCESSABLE_ENTITY
}

pub async fn create_repository_webhook(
    app_id: &str,
    private_key_pem: &str,
    installation_id: i64,
    repo_full_name: &str,
    webhook_url: &str,
    webhook_secret: &str,
) -> ApiResult<()> {
    create_repository_webhook_with_client(
        &HTTP_CLIENT,
        "https://api.github.com",
        app_id,
        private_key_pem,
        installation_id,
        repo_full_name,
        webhook_url,
        webhook_secret,
    )
    .await
}

#[derive(Debug, Deserialize)]
pub struct GithubCommitResponse {
    pub sha: String,
    pub commit: CommitDetail,
}

#[derive(Debug, Deserialize)]
pub struct CommitDetail {
    pub message: String,
}

async fn get_repo_latest_commit_with_client(
    client: &reqwest::Client,
    api_base_url: &str,
    app_id: &str,
    private_key_pem: &str,
    installation_id: i64,
    repo_full_name: &str,
    branch: &str,
) -> ApiResult<crate::domain::app::GitMetadata> {
    let token = get_installation_token_with_client(client, api_base_url, app_id, private_key_pem, installation_id).await?;

    let response = client
        .get(format!("{}/repos/{}/commits/{}", api_base_url, repo_full_name, branch))
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "mikrom-api")
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| ApiError::Internal(format!("GitHub API request failed: {}", e)))?;

    if !response.status().is_success() {
        let status = response.status();
        let error_body = response.text().await.unwrap_or_default();
        return Err(ApiError::Internal(format!(
            "Failed to fetch latest commit: {} - {}",
            status, error_body
        )));
    }

    let commit_resp: GithubCommitResponse = response
        .json()
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to parse commit response: {}", e)))?;

    Ok(crate::domain::app::GitMetadata {
        git_commit_hash: Some(commit_resp.sha),
        git_commit_message: Some(commit_resp.commit.message),
        git_branch: Some(branch.to_string()),
    })
}

pub async fn get_repo_latest_commit(
    app_id: &str,
    private_key_pem: &str,
    installation_id: i64,
    repo_full_name: &str,
    branch: &str,
) -> ApiResult<crate::domain::app::GitMetadata> {
    get_repo_latest_commit_with_client(
        &HTTP_CLIENT,
        "https://api.github.com",
        app_id,
        private_key_pem,
        installation_id,
        repo_full_name,
        branch,
    )
    .await
}

pub async fn list_user_installations(
    app_id: &str,
    private_key_pem: &str,
) -> ApiResult<Vec<serde_json::Value>> {
    let jwt = generate_jwt(app_id, private_key_pem)?;

    let response = HTTP_CLIENT
        .get("https://api.github.com/app/installations")
        .header("Authorization", format!("Bearer {}", jwt))
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "mikrom-api")
        .send()
        .await
        .map_err(|e| ApiError::Internal(format!("GitHub API request failed: {}", e)))?;

    if !response.status().is_success() {
        let status = response.status();
        let error_body = response.text().await.unwrap_or_default();
        return Err(ApiError::Internal(format!(
            "Failed to list user installations: {} - {}",
            status, error_body
        )));
    }

    let installations: Vec<serde_json::Value> = response.json().await.map_err(|e| {
        ApiError::Internal(format!("Failed to parse installations response: {}", e))
    })?;

    Ok(installations)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webhook_create_status_422_is_idempotent() {
        assert!(webhook_create_status_is_idempotent(
            reqwest::StatusCode::UNPROCESSABLE_ENTITY
        ));
        assert!(!webhook_create_status_is_idempotent(reqwest::StatusCode::CONFLICT));
    }
}
