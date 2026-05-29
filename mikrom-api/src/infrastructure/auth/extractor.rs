use crate::error::ApiError;
use axum::{
    extract::{FromRef, FromRequestParts},
    http::{HeaderMap, Uri, request::Parts},
};

#[derive(Clone, Debug)]
pub struct AuthUser {
    pub user_id: String,
    pub email: String,
    pub role: crate::domain::UserRole,
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    crate::AppState: FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let state = crate::AppState::from_ref(state);
        let claims = parts.extensions.get::<crate::auth::jwt::Claims>().cloned();

        let claims = if let Some(claims) = claims {
            claims
        } else {
            // 1. Get token from Authorization header OR query parameter (for SSE)
            let token = extract_token_from_headers_and_uri(&parts.headers, &parts.uri)?;

            // 2. Decode and validate JWT
            crate::auth::jwt::verify_token(&token, &state.jwt_secret)
                .map_err(|_| ApiError::Auth("Invalid or expired token".into()))?
        };

        Ok(auth_user_from_claims(claims))
    }
}

impl rovo::aide::OperationInput for AuthUser {
    fn operation_input(
        _ctx: &mut rovo::aide::generate::GenContext,
        operation: &mut rovo::aide::openapi::Operation,
    ) {
        operation.security.push(indexmap::indexmap! {
            "jwt".to_string() => vec![]
        });
    }
}
fn auth_user_from_claims(claims: crate::auth::jwt::Claims) -> AuthUser {
    AuthUser {
        user_id: claims.sub,
        email: claims.email,
        role: claims.role,
    }
}

pub(crate) fn extract_token_from_headers_and_uri(
    headers: &HeaderMap,
    uri: &Uri,
) -> Result<String, ApiError> {
    if let Some(auth_header) = headers.get("Authorization").and_then(|h| h.to_str().ok()) {
        if !auth_header.starts_with("Bearer ") {
            return Err(ApiError::Auth("Invalid authorization header format".into()));
        }

        return Ok(auth_header[7..].to_string());
    }

    uri.query()
        .and_then(|q| {
            q.split('&')
                .find(|pair| pair.starts_with("token="))
                .and_then(|pair| pair.get(6..))
                .map(|s| s.to_string())
        })
        .ok_or_else(|| {
            ApiError::Auth("Missing authorization header or token query parameter".into())
        })
}

pub struct AdminUser(pub AuthUser);

impl<S> FromRequestParts<S> for AdminUser
where
    S: Send + Sync,
    crate::AppState: FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth_user = AuthUser::from_request_parts(parts, state).await?;

        if auth_user.role != crate::domain::UserRole::Admin {
            return Err(ApiError::Forbidden);
        }

        Ok(AdminUser(auth_user))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppState;
    use crate::domain::{
        MockAppRepository, MockDatabaseRepository, MockGithubRepository, MockScheduler,
        MockUserRepository, MockVolumeRepository, UserRole,
    };
    use axum::http::Request;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_auth_extractor_success() {
        let user_id = uuid::Uuid::new_v4().to_string();
        let email = "test@example.com".to_string();
        let jwt_secret = "test-secret".to_string();
        let token =
            crate::auth::jwt::create_token(&user_id, &email, &UserRole::User, &jwt_secret).unwrap();

        let state = AppState {
            ctx: crate::application::ApiContext::default(),
            user_repo: Arc::new(MockUserRepository::new()),
            app_repo: Arc::new(MockAppRepository::new()),
            database_repo: Arc::new(MockDatabaseRepository::new()),
            github_repo: Arc::new(MockGithubRepository::new()),
            volume_repo: Arc::new(MockVolumeRepository::new()),
            scheduler: Arc::new(MockScheduler::new()),
            nats: crate::nats::TypedNatsClient::new_custom(Arc::new(
                crate::nats::MockNatsClient::new(),
            )),
            router_addr: "http://localhost:8080".to_string(),
            frontend_url: "http://localhost:3000".to_string(),
            api_db: sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://localhost/dummy")
                .unwrap(),
            jwt_secret,
            master_key: "key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            workspace_events: tokio::sync::broadcast::channel(1).0,
            mesh_status:
                tokio::sync::watch::channel(crate::application::vms::MeshStatus::default()).0,
            acme_email: "admin@mikrom.spluca.org".to_string(),
            acme_staging: true,
            acme_check_interval: 3600,
            github_app_id: None,
            github_private_key: None,
            github_app_slug: None,
            github_webhook_url_base: None,
            active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
        };

        let request = Request::builder()
            .header("Authorization", format!("Bearer {}", token))
            .body(())
            .unwrap();

        let (mut parts, _) = request.into_parts();
        let auth_user = AuthUser::from_request_parts(&mut parts, &state)
            .await
            .unwrap();

        assert_eq!(auth_user.user_id, user_id);
        assert_eq!(auth_user.email, email);
    }
}
