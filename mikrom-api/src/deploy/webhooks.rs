use crate::AppState;
use crate::error::{ApiError, ApiResult};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;
use tracing::{error, info, warn};

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Deserialize)]
pub struct GitHubPushEvent {
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub repository: GitHubRepository,
}

#[derive(Debug, Deserialize)]
pub struct GitHubRepository {
    pub name: String,
    pub full_name: String,
    pub html_url: String,
}

pub async fn github_webhook_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    body_bytes: axum::body::Bytes,
) -> ApiResult<StatusCode> {
    // 1. Filter Event Type early
    let event_type = headers
        .get("x-github-event")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    if event_type != "push" {
        info!("Ignoring GitHub event type: {}", event_type);
        return Ok(StatusCode::OK);
    }

    // 2. Parse Payload early to find the application
    let payload: GitHubPushEvent = serde_json::from_slice(&body_bytes).map_err(|e| {
        error!("Failed to parse GitHub webhook payload: {}", e);
        ApiError::BadRequest(format!("Invalid JSON: {e}"))
    })?;

    // 3. Check branch (only main/master)
    if payload.ref_name != "refs/heads/main" && payload.ref_name != "refs/heads/master" {
        info!("Ignoring push to non-main branch: {}", payload.ref_name);
        return Ok(StatusCode::OK);
    }

    // 4. Find Application to get its specific secret
    let app = if let Some(a) = state
        .app_repo
        .get_app_by_name(&payload.repository.name)
        .await?
    {
        Some(a)
    } else {
        state
            .app_repo
            .get_app_by_name(&payload.repository.full_name)
            .await?
    };

    let app = match app {
        Some(a) => a,
        None => {
            warn!(
                "No application found matching repository: {}",
                payload.repository.full_name
            );
            return Ok(StatusCode::OK);
        },
    };

    // 5. Validate Signature using app-specific secret
    let signature_header = headers
        .get("x-hub-signature-256")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| {
            warn!("Missing x-hub-signature-256 header");
            ApiError::Auth("Missing signature".into())
        })?;

    if !signature_header.starts_with("sha256=") {
        return Err(ApiError::Auth("Invalid signature format".into()));
    }

    let signature_hex = &signature_header[7..];
    let expected_signature =
        hex::decode(signature_hex).map_err(|_| ApiError::Auth("Invalid hex signature".into()))?;

    let secret = app.github_webhook_secret.as_deref().ok_or_else(|| {
        error!(app_name = %app.name, "Application has no webhook secret configured");
        ApiError::Internal("App webhook secret not configured".into())
    })?;

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|e| ApiError::Internal(format!("HMAC initialization failed: {e}")))?;
    mac.update(&body_bytes);

    if mac.verify_slice(&expected_signature).is_err() {
        warn!(app_name = %app.name, "GitHub webhook signature verification failed");
        return Err(ApiError::Auth("Invalid signature".into()));
    }

    info!(app_name = %app.name, "GitHub trigger detected and verified, initiating auto-deploy");

    // 6. Trigger Build
    let state_clone = state.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::deploy::handlers::trigger_app_build(state_clone, app).await {
            error!("Background auto-deploy failed: {}", e);
        }
    });

    Ok(StatusCode::ACCEPTED)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::app::App;
    use crate::repositories::app_repository::MockAppRepository;
    use crate::repositories::user_repository::MockUserRepository;
    use chrono::Utc;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    use std::sync::Arc;
    use uuid::Uuid;

    fn create_test_state(app_repo: MockAppRepository) -> AppState {
        AppState {
            user_repo: Arc::new(MockUserRepository::new()),
            app_repo: Arc::new(app_repo),
            scheduler_client: None,
            scheduler_config: crate::scheduler::SchedulerConfig::default(),
            builder_addr: "http://localhost:5004".into(),
            jwt_secret: "secret".into(),
            master_key: "key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
        }
    }

    fn compute_signature(secret: &str, body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let result = mac.finalize().into_bytes();
        format!("sha256={}", hex::encode(result))
    }

    #[tokio::test]
    async fn test_github_webhook_invalid_signature() {
        let mut mock_repo = MockAppRepository::new();
        let app_name = "test-repo";
        let secret = "correct-secret";

        let app = App {
            id: Uuid::new_v4(),
            name: app_name.to_string(),
            git_url: "https://github.com/owner/test-repo".into(),
            port: 8080,
            hostname: None,
            user_id: Uuid::new_v4(),
            github_webhook_secret: Some(secret.to_string()),
            active_deployment_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        mock_repo
            .expect_get_app_by_name()
            .returning(move |_| Ok(Some(app.clone())));

        let state = create_test_state(mock_repo);
        let body = r#"{"ref": "refs/heads/main", "repository": {"name": "test-repo", "full_name": "owner/test-repo", "html_url": "..."}}"#;
        let mut headers = HeaderMap::new();
        headers.insert("x-github-event", "push".parse().unwrap());
        headers.insert(
            "x-hub-signature-256",
            "sha256=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .parse()
                .unwrap(),
        );

        let result = github_webhook_handler(State(state), headers, body.into()).await;

        match result {
            Err(ApiError::Auth(msg)) => assert_eq!(msg, "Invalid signature"),
            _ => panic!("Expected Auth error, got {:?}", result),
        }
    }

    #[tokio::test]
    async fn test_github_webhook_wrong_event() {
        let mock_repo = MockAppRepository::new();
        let state = create_test_state(mock_repo);

        let mut headers = HeaderMap::new();
        headers.insert("x-github-event", "ping".parse().unwrap());

        let result = github_webhook_handler(State(state), headers, "{}".into())
            .await
            .unwrap();
        assert_eq!(result, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_github_webhook_wrong_branch() {
        let mock_repo = MockAppRepository::new();
        let state = create_test_state(mock_repo);

        let body = r#"{"ref": "refs/heads/feature", "repository": {"name": "repo", "full_name": "owner/repo", "html_url": "..."}}"#;
        let mut headers = HeaderMap::new();
        headers.insert("x-github-event", "push".parse().unwrap());

        let result = github_webhook_handler(State(state), headers, body.into())
            .await
            .unwrap();
        assert_eq!(result, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_github_webhook_success() {
        let mut mock_repo = MockAppRepository::new();
        let app_name = "test-repo";
        let secret = "my-secret";

        let app = App {
            id: Uuid::new_v4(),
            name: app_name.to_string(),
            git_url: "https://github.com/owner/test-repo".into(),
            port: 8080,
            hostname: None,
            user_id: Uuid::new_v4(),
            github_webhook_secret: Some(secret.to_string()),
            active_deployment_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        mock_repo
            .expect_get_app_by_name()
            .with(mockall::predicate::eq(app_name))
            .returning(move |_| Ok(Some(app.clone())));

        let state = create_test_state(mock_repo);
        let body = r#"{"ref": "refs/heads/main", "repository": {"name": "test-repo", "full_name": "owner/test-repo", "html_url": "..."}}"#;
        let signature = compute_signature(secret, body.as_bytes());

        let mut headers = HeaderMap::new();
        headers.insert("x-github-event", "push".parse().unwrap());
        headers.insert("x-hub-signature-256", signature.parse().unwrap());

        let result = github_webhook_handler(State(state), headers, body.into())
            .await
            .unwrap();
        assert_eq!(result, StatusCode::ACCEPTED);
    }
}
