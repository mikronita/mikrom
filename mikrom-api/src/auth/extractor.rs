use crate::error::ApiError;
use axum::{
    async_trait,
    extract::{FromRef, FromRequestParts},
    http::request::Parts,
};

#[derive(Clone, Debug)]
pub struct AuthUser {
    pub user_id: String,
    pub email: String,
    pub role: crate::repositories::user_repository::UserRole,
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    crate::AppState: FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let state = crate::AppState::from_ref(state);

        // 1. Get token from Authorization header OR query parameter (for SSE)
        let token = if let Some(auth_header) = parts
            .headers
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
        {
            if !auth_header.starts_with("Bearer ") {
                return Err(ApiError::Auth("Invalid authorization header format".into()));
            }
            auth_header[7..].to_string()
        } else {
            // Fallback to query parameter "token" (for SSE)
            parts
                .uri
                .query()
                .and_then(|q| {
                    q.split('&')
                        .find(|pair| pair.starts_with("token="))
                        .and_then(|pair| pair.get(6..))
                        .map(|s| s.to_string())
                })
                .ok_or_else(|| {
                    ApiError::Auth("Missing authorization header or token query parameter".into())
                })?
        };

        // 2. Decode and validate JWT
        let claims = crate::auth::jwt::verify_token(&token, &state.jwt_secret)
            .map_err(|_| ApiError::Auth("Invalid or expired token".into()))?;

        Ok(AuthUser {
            user_id: claims.sub,
            email: claims.email,
            role: claims.role,
        })
    }
}

pub struct AdminUser(pub AuthUser);

#[async_trait]
impl<S> FromRequestParts<S> for AdminUser
where
    S: Send + Sync,
    crate::AppState: FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth_user = AuthUser::from_request_parts(parts, state).await?;

        if auth_user.role != crate::repositories::user_repository::UserRole::Admin {
            return Err(ApiError::Forbidden);
        }

        Ok(AdminUser(auth_user))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppState;
    use crate::repositories::app_repository::MockAppRepository;
    use crate::repositories::user_repository::{MockUserRepository, UserRole};
    use axum::http::Request;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_auth_extractor_success() {
        let user_id = uuid::Uuid::new_v4().to_string();
        let email = "test@example.com".to_string();
        let jwt_secret = "test-secret".to_string();
        let token =
            crate::auth::jwt::create_token(&user_id, &email, &UserRole::User, &jwt_secret).unwrap();

        let nats_url =
            std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
        let nats_client = async_nats::connect(nats_url).await.unwrap();
        let nats = crate::nats::TypedNatsClient::new(nats_client);
        let state = AppState {
            user_repo: Arc::new(MockUserRepository::new()),
            app_repo: Arc::new(MockAppRepository::new()),
            scheduler: Arc::new(crate::scheduler::MockScheduler::new()),
            nats,
            router_addr: "http://localhost:8080".to_string(),
            frontend_url: "http://localhost:3000".to_string(),
            api_db: sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://localhost/dummy")
                .unwrap(),
            jwt_secret,
            master_key: "key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            acme_email: "admin@mikrom.spluca.org".to_string(),
            acme_staging: true,
            acme_check_interval: 3600,
            github_repo: Arc::new(crate::repositories::MockGithubRepository::default()),
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
