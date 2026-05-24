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
    pub repository: GitHubRepository,
    pub head_commit: Option<GitHubCommit>,
}

#[derive(Debug, Deserialize)]
pub struct GitHubRepository {
    pub id: i64,
}

#[derive(Debug, Deserialize)]
pub struct GitHubCommit {
    pub id: String,
    pub message: String,
}

#[rovo::rovo]
pub async fn github_webhook_handler(
    state: State<AppState>,
    Path(app_name): Path<String>,
    headers: HeaderMap,
    body_bytes: axum::body::Bytes,
) -> ApiResult<StatusCode> {
    tracing::debug!(%app_name, "Received request on /webhooks/github/:app_name");
    github_webhook_handler_core(state, Some(app_name), headers, body_bytes).await
}

#[rovo::rovo]
pub async fn github_webhook_handler_generic(
    state: State<AppState>,
    headers: HeaderMap,
    body_bytes: axum::body::Bytes,
) -> ApiResult<StatusCode> {
    tracing::debug!("Received request on generic /webhooks/github");
    github_webhook_handler_core(state, None, headers, body_bytes).await
}

async fn github_webhook_handler_core(
    State(state): State<AppState>,
    app_name_path: Option<String>,
    headers: HeaderMap,
    body_bytes: axum::body::Bytes,
) -> ApiResult<StatusCode> {
    tracing::info!(?app_name_path, "Processing GitHub webhook...");
    // 1. Filter Event Type early
    let event_type = headers
        .get("x-github-event")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    tracing::debug!(%event_type, "Event type detected");

    if event_type == "ping" {
        return Ok(StatusCode::OK);
    }

    if event_type != "push" {
        info!("Ignoring GitHub event type: {}", event_type);
        return Ok(StatusCode::OK);
    }

    // 2. Identify Application (via Path or Payload)
    let app = if let Some(app_name) = app_name_path {
        tracing::debug!(%app_name, "Looking up app by name from path");
        state
            .app_repo
            .get_app_by_name(&app_name)
            .await?
            .ok_or_else(|| {
                warn!(%app_name, "Webhook received for non-existent application (path-based)");
                ApiError::NotFound("Application not found".into())
            })?
    } else {
        tracing::debug!("Generic endpoint: parsing payload to find repo ID");
        // Generic endpoint: we must parse the payload first to find the repo ID
        // We do a partial, unverified parse just for ID lookup.
        // We verify the HMAC immediately after finding the secret.
        let payload: GitHubPushEvent = serde_json::from_slice(&body_bytes).map_err(|e| {
            warn!("Failed to pre-parse GitHub webhook payload: {}", e);
            ApiError::BadRequest("Invalid payload format".into())
        })?;

        tracing::debug!(
            repo_id = payload.repository.id,
            "Looking up app by GitHub repo ID"
        );
        state
            .app_repo
            .get_app_by_github_repo_id(payload.repository.id)
            .await?
            .ok_or_else(|| {
                warn!(repo_id = %payload.repository.id, "Webhook received for non-existent repository ID");
                ApiError::NotFound("Application not found for this repository".into())
            })?
    };

    tracing::info!(app_name = %app.name, "Found application for webhook");

    // 3. Validate Signature BEFORE parsing fully
    let signature_header = headers
        .get("x-hub-signature-256")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| {
            warn!(app_name = %app.name, "Missing x-hub-signature-256 header");
            ApiError::Auth("Missing signature".into())
        })?;

    tracing::debug!(%signature_header, "Verifying signature");

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

    // 4. Now that signature is verified, parse payload properly
    let payload: GitHubPushEvent = serde_json::from_slice(&body_bytes).map_err(|e| {
        error!(app_name = %app.name, "Failed to parse GitHub webhook payload after verification: {}", e);
        ApiError::BadRequest("Invalid payload format".into())
    })?;

    // 5. Check branch (only main/master)
    if payload.ref_name != "refs/heads/main" && payload.ref_name != "refs/heads/master" {
        info!(app_name = %app.name, branch = %payload.ref_name, "Ignoring push to non-main branch");
        return Ok(StatusCode::OK);
    }

    info!(app_name = %app.name, "GitHub trigger detected and verified, initiating auto-deploy");

    let git_metadata = payload
        .head_commit
        .map(|commit| crate::domain::GitMetadata {
            git_commit_hash: Some(commit.id),
            git_commit_message: Some(commit.message),
            git_branch: Some(payload.ref_name.replace("refs/heads/", "")),
        });

    // 6. Trigger Build
    let state_clone = state.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::application::deployment::DeploymentService::trigger_app_build(
            &state_clone,
            &app,
            git_metadata.as_ref(),
        )
        .await
        {
            error!("Background auto-deploy failed: {}", e);
        }
    });

    Ok(StatusCode::ACCEPTED)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::App;
    use crate::domain::MockAppRepository;
    use crate::domain::user::MockUserRepository;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    use std::sync::Arc;
    use uuid::Uuid;
    async fn create_test_state(app_repo: MockAppRepository) -> AppState {
        let nats_url =
            std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
        let nats_client = async_nats::connect(nats_url).await.unwrap();
        let nats = crate::nats::TypedNatsClient::new(nats_client);
        AppState {
            ctx: crate::application::ApiContext::default(),
            user_repo: Arc::new(MockUserRepository::new()),
            app_repo: Arc::new(app_repo),
            github_repo: Arc::new(crate::domain::github::MockGithubRepository::default()),
            volume_repo: Arc::new(crate::domain::MockVolumeRepository::new()),
            scheduler: Arc::new(crate::domain::MockScheduler::new()),
            nats,
            router_addr: "http://localhost:8080".into(),
            frontend_url: "http://localhost:3000".into(),
            api_db: sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://localhost/dummy")
                .unwrap(),
            jwt_secret: "secret".into(),
            master_key: "key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            workspace_events: tokio::sync::broadcast::channel(1).0,
            mesh_status:
                tokio::sync::watch::channel(crate::application::vms::MeshStatus::default()).0,
            acme_email: "admin@mikrom.spluca.org".into(),
            acme_staging: true,
            acme_check_interval: 3600,
            github_app_id: None,
            github_private_key: None,
            github_app_slug: None,
            github_webhook_url_base: None,
            active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
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
            port: crate::domain::types::Port::new(8080).unwrap(),
            github_webhook_secret: Some(secret.to_string()),
            ..App::default()
        };

        mock_repo
            .expect_get_app_by_name()
            .with(mockall::predicate::eq("test-app"))
            .returning(move |_| Ok(Some(app.clone())));

        let state = create_test_state(mock_repo).await;

        let body = r#"{"ref": "refs/heads/main", "repository": {"id": 12345}}"#;
        let mut headers = HeaderMap::new();
        headers.insert("x-github-event", "push".parse().unwrap());
        headers.insert(
            "x-hub-signature-256",
            "sha256=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .parse()
                .unwrap(),
        );

        let result = __github_webhook_handler_impl(
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

        let result = __github_webhook_handler_impl(
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
            port: crate::domain::types::Port::new(8080).unwrap(),
            github_webhook_secret: Some(secret.to_string()),
            ..App::default()
        };

        mock_repo
            .expect_get_app_by_name()
            .returning(move |_| Ok(Some(app.clone())));

        let state = create_test_state(mock_repo).await;

        let body = r#"{"ref": "refs/heads/feature", "repository": {"id": 12345}}"#;
        let signature = compute_signature(secret, body.as_bytes());
        let mut headers = HeaderMap::new();
        headers.insert("x-github-event", "push".parse().unwrap());
        headers.insert("x-hub-signature-256", signature.parse().unwrap());

        let result = __github_webhook_handler_impl(
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
            port: crate::domain::types::Port::new(8080).unwrap(),
            github_webhook_secret: Some(secret.to_string()),
            ..App::default()
        };

        mock_repo
            .expect_get_app_by_name()
            .with(mockall::predicate::eq("test-repo"))
            .returning(move |_| Ok(Some(app.clone())));

        let state = create_test_state(mock_repo).await;

        let body = r#"{"ref": "refs/heads/main", "repository": {"id": 12345}}"#;
        let signature = compute_signature(secret, body.as_bytes());

        let mut headers = HeaderMap::new();
        headers.insert("x-github-event", "push".parse().unwrap());
        headers.insert("x-hub-signature-256", signature.parse().unwrap());

        let result = __github_webhook_handler_impl(
            State(state),
            Path("test-repo".to_string()),
            headers,
            body.into(),
        )
        .await
        .unwrap();
        assert_eq!(result, StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn test_github_webhook_generic_success() {
        let mut mock_repo = MockAppRepository::new();
        let app_id = Uuid::new_v4();
        let secret = "my-secret";
        let repo_id = 98765;

        let app = App {
            id: app_id,
            name: "generic-app".to_string(),
            git_url: "https://github.com/owner/generic-repo".into(),
            port: crate::domain::types::Port::new(8080).unwrap(),
            github_webhook_secret: Some(secret.to_string()),
            github_repo_id: Some(repo_id),
            github_repo_full_name: Some("owner/generic-repo".to_string()),
            ..App::default()
        };

        mock_repo
            .expect_get_app_by_github_repo_id()
            .with(mockall::predicate::eq(repo_id))
            .returning(move |_| Ok(Some(app.clone())));

        let state = create_test_state(mock_repo).await;

        let body = format!(
            r#"{{"ref": "refs/heads/main", "repository": {{"id": {}}}}}"#,
            repo_id
        );
        let signature = compute_signature(secret, body.as_bytes());

        let mut headers = HeaderMap::new();
        headers.insert("x-github-event", "push".parse().unwrap());
        headers.insert("x-hub-signature-256", signature.parse().unwrap());

        let result = __github_webhook_handler_generic_impl(State(state), headers, body.into())
            .await
            .unwrap();
        assert_eq!(result, StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn test_github_webhook_with_commit_metadata() {
        let mut mock_repo = MockAppRepository::new();
        let app_id = Uuid::new_v4();
        let secret = "my-secret";
        let repo_id = 12345;

        let app = App {
            id: app_id,
            name: "metadata-app".to_string(),
            git_url: "https://github.com/owner/metadata-repo".into(),
            port: crate::domain::types::Port::new(8080).unwrap(),
            github_webhook_secret: Some(secret.to_string()),
            github_repo_id: Some(repo_id),
            github_repo_full_name: Some("owner/metadata-repo".to_string()),
            ..App::default()
        };

        mock_repo
            .expect_get_app_by_github_repo_id()
            .with(mockall::predicate::eq(repo_id))
            .returning(move |_| Ok(Some(app.clone())));

        let state = create_test_state(mock_repo).await;

        let body = format!(
            r#"{{"ref": "refs/heads/main", "repository": {{"id": {}}}, "head_commit": {{"id": "abc12345", "message": "feat: test commit"}}}}"#,
            repo_id
        );
        let signature = compute_signature(secret, body.as_bytes());

        let mut headers = HeaderMap::new();
        headers.insert("x-github-event", "push".parse().unwrap());
        headers.insert("x-hub-signature-256", signature.parse().unwrap());

        let result = __github_webhook_handler_generic_impl(State(state), headers, body.into())
            .await
            .unwrap();

        assert_eq!(result, StatusCode::ACCEPTED);
        // Note: The actual background task that calls trigger_app_build is spawned via tokio::spawn.
        // Verifying the metadata was passed would require more complex mocking of trigger_app_build or a test of that function directly.
        // Here we at least verify the payload parses and the handler proceeds.
    }
}
