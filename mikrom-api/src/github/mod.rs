pub mod handlers;

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

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
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

    let key = EncodingKey::from_rsa_pem(private_key_pem.as_bytes())
        .map_err(|e| ApiError::Internal(format!("Invalid private key: {}", e)))?;

    jsonwebtoken::encode(&Header::new(jsonwebtoken::Algorithm::RS256), &claims, &key)
        .map_err(|e| ApiError::Internal(format!("Failed to encode JWT: {}", e)))
}

pub async fn get_installation_token(
    app_id: &str,
    private_key_pem: &str,
    installation_id: i64,
) -> ApiResult<String> {
    let jwt = generate_jwt(app_id, private_key_pem)?;

    let response = HTTP_CLIENT
        .post(format!(
            "https://api.github.com/app/installations/{}/access_tokens",
            installation_id
        ))
        .header("Authorization", format!("Bearer {}", jwt))
        .header("Accept", "application/vnd.github.v3+json")
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

pub async fn list_installation_repos(
    app_id: &str,
    private_key_pem: &str,
    installation_id: i64,
) -> ApiResult<Vec<GithubRepo>> {
    let token = get_installation_token(app_id, private_key_pem, installation_id).await?;
    let mut all_repos = Vec::new();
    let mut page = 1;

    loop {
        let url = format!(
            "https://api.github.com/installation/repositories?per_page=100&page={}",
            page
        );
        let response = HTTP_CLIENT
            .get(&url)
            .header("Authorization", format!("token {}", token))
            .header("Accept", "application/vnd.github.v3+json")
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
