use crate::AppState;
use crate::error::{ApiError, ApiResult};
use axum::{
    extract::{Path, State},
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
}

#[utoipa::path(
    post,
    path = "/webhooks/github/{app_name}",
    request_body(content = String, description = "GitHub Webhook Payload", content_type = "application/json"),
    params(
        ("app_name" = String, Path, description = "Application Name")
    ),
    responses(
        (status = 200, description = "Webhook ignored or processed successfully without deployment"),
        (status = 202, description = "Webhook accepted and deployment initiated"),
        (status = 400, description = "Bad Request"),
        (status = 401, description = "Unauthorized (Invalid Signature)"),
        (status = 404, description = "Application not found")
    ),
    tag = "system"
)]
pub async fn github_webhook_handler(
    State(state): State<AppState>,
    Path(app_name): Path<String>,
    headers: HeaderMap,
    body_bytes: axum::body::Bytes,
) -> ApiResult<StatusCode> {
    // 1. Filter Event Type early
    let event_type = headers
        .get("x-github-event")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    if event_type == "ping" {
        return Ok(StatusCode::OK);
    }

    if event_type != "push" {
        info!("Ignoring GitHub event type: {}", event_type);
        return Ok(StatusCode::OK);
    }

    // 2. Find Application by Name
    let app = state
        .app_repo
        .get_app_by_name(&app_name)
        .await?
        .ok_or_else(|| {
            warn!(%app_name, "Webhook received for non-existent application");
            ApiError::NotFound("Application not found".into())
        })?;

    // 3. Validate Signature BEFORE parsing
    let signature_header = headers
        .get("x-hub-signature-256")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| {
            warn!(app_name = %app.name, "Missing x-hub-signature-256 header");
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
        .map_err(|_| ApiError::Internal("HMAC initialization failed".into()))?;
    mac.update(&body_bytes);

    if mac.verify_slice(&expected_signature).is_err() {
        warn!(app_name = %app.name, "GitHub webhook signature verification failed");
        return Err(ApiError::Auth("Invalid signature".into()));
    }

    // 4. Now that signature is verified, parse payload
    let payload: GitHubPushEvent = serde_json::from_slice(&body_bytes).map_err(|e| {
        error!(app_name = %app.name, "Failed to parse GitHub webhook payload: {}", e);
        ApiError::BadRequest("Invalid payload format".into())
    })?;

    // 5. Check branch (only main/master)
    if payload.ref_name != "refs/heads/main" && payload.ref_name != "refs/heads/master" {
        info!(app_name = %app.name, branch = %payload.ref_name, "Ignoring push to non-main branch");
        return Ok(StatusCode::OK);
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
    async fn create_test_state(app_repo: MockAppRepository) -> AppState {
        let nats_client = async_nats::connect("nats://localhost:4222").await.unwrap();
        AppState {
            user_repo: Arc::new(MockUserRepository::new()),
            app_repo: Arc::new(app_repo),
            scheduler: Arc::new(crate::scheduler::MockScheduler::new()),
            nats_client,
            router_addr: "http://localhost:8080".into(),
            jwt_secret: "secret".into(),
            master_key: "key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            build_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
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
        let app_id = Uuid::new_v4();
        let secret = "correct-secret";

        let app = App {
            id: app_id,
            name: "test-app".to_string(),
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
            .with(mockall::predicate::eq("test-app"))
            .returning(move |_| Ok(Some(app.clone())));

        let state = create_test_state(mock_repo).await;

        let body = r#"{"ref": "refs/heads/main"}"#;
        let mut headers = HeaderMap::new();
        headers.insert("x-github-event", "push".parse().unwrap());
        headers.insert(
            "x-hub-signature-256",
            "sha256=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .parse()
                .unwrap(),
        );

        let result = github_webhook_handler(
            State(state),
            Path("test-app".to_string()),
            headers,
            body.into(),
        )
        .await;

        match result {
            Err(ApiError::Auth(msg)) => assert_eq!(msg, "Invalid signature"),
            _ => panic!("Expected Auth error, got {:?}", result),
        }
    }

    #[tokio::test]
    async fn test_github_webhook_wrong_event() {
        let mock_repo = MockAppRepository::new();
        let state = create_test_state(mock_repo).await;

        let mut headers = HeaderMap::new();
        headers.insert("x-github-event", "ping".parse().unwrap());

        let result = github_webhook_handler(
            State(state),
            Path("any-app".to_string()),
            headers,
            "{}".into(),
        )
        .await
        .unwrap();
        assert_eq!(result, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_github_webhook_wrong_branch() {
        let mut mock_repo = MockAppRepository::new();
        let app_id = Uuid::new_v4();
        let secret = "secret";

        let app = App {
            id: app_id,
            name: "test-app".to_string(),
            git_url: "".into(),
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

        let state = create_test_state(mock_repo).await;

        let body = r#"{"ref": "refs/heads/feature"}"#;
        let signature = compute_signature(secret, body.as_bytes());
        let mut headers = HeaderMap::new();
        headers.insert("x-github-event", "push".parse().unwrap());
        headers.insert("x-hub-signature-256", signature.parse().unwrap());

        let result = github_webhook_handler(
            State(state),
            Path("test-app".to_string()),
            headers,
            body.into(),
        )
        .await
        .unwrap();
        assert_eq!(result, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_github_webhook_success() {
        let mut mock_repo = MockAppRepository::new();
        let app_id = Uuid::new_v4();
        let secret = "my-secret";

        let app = App {
            id: app_id,
            name: "test-repo".to_string(),
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
            .with(mockall::predicate::eq("test-repo"))
            .returning(move |_| Ok(Some(app.clone())));

        let state = create_test_state(mock_repo).await;

        let body = r#"{"ref": "refs/heads/main"}"#;
        let signature = compute_signature(secret, body.as_bytes());

        let mut headers = HeaderMap::new();
        headers.insert("x-github-event", "push".parse().unwrap());
        headers.insert("x-hub-signature-256", signature.parse().unwrap());

        let result = github_webhook_handler(
            State(state),
            Path("test-repo".to_string()),
            headers,
            body.into(),
        )
        .await
        .unwrap();
        assert_eq!(result, StatusCode::ACCEPTED);
    }
}
