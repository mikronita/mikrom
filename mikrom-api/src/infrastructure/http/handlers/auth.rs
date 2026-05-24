use crate::AppState;
use crate::application::auth::{AuthResult, AuthService, RegisterParams};
use crate::domain::User;
use crate::error::ApiResult;
use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

#[derive(Debug, Serialize, rovo::schemars::JsonSchema)]
pub struct AuthResponse {
    pub user: UserResponse,
    pub token: String,
}

impl From<AuthResult> for AuthResponse {
    fn from(result: AuthResult) -> Self {
        Self {
            user: result.user.into(),
            token: result.token,
        }
    }
}

#[derive(Debug, Serialize, rovo::schemars::JsonSchema)]
pub struct UserResponse {
    pub id: String,
    pub email: String,
    pub role: crate::domain::UserRole,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub vpc_ipv6_prefix: Option<String>,
}

impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        Self {
            id: user.id.to_string(),
            email: user.email,
            role: user.role,
            first_name: user.first_name,
            last_name: user.last_name,
            vpc_ipv6_prefix: user.vpc_ipv6_prefix,
        }
    }
}

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct UpdateProfileRequest {
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

#[rovo::rovo]
pub async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegisterRequest>,
) -> ApiResult<(StatusCode, Json<AuthResponse>)> {
    info!(email = %payload.email, "Registering new user");

    let result = AuthService::register(
        &state,
        RegisterParams {
            email: payload.email,
            password: payload.password,
            first_name: payload.first_name,
            last_name: payload.last_name,
        },
    )
    .await?;

    Ok((StatusCode::CREATED, Json(result.into())))
}

#[rovo::rovo]
pub async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> ApiResult<Json<AuthResponse>> {
    info!(email = %payload.email, "User login attempt");

    let result = AuthService::login(&state, payload.email, payload.password).await?;

    Ok(Json(result.into()))
}

#[rovo::rovo]
pub async fn get_profile(
    auth: crate::AuthUser,
    State(state): State<AppState>,
) -> ApiResult<Json<UserResponse>> {
    let user = AuthService::get_profile_by_auth(&state, &auth.user_id).await?;

    Ok(Json(user.into()))
}

#[rovo::rovo]
pub async fn update_profile(
    auth: crate::AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<UpdateProfileRequest>,
) -> ApiResult<Json<UserResponse>> {
    let user = AuthService::update_profile_by_auth(
        &state,
        &auth.user_id,
        payload.first_name,
        payload.last_name,
    )
    .await?;

    Ok(Json(user.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::github::MockGithubRepository;
    use crate::domain::{MockAppRepository, MockUserRepository, MockVolumeRepository, User};
    use std::sync::Arc;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_register_success() {
        let mut mock_repo = MockUserRepository::new();
        let email = "test@example.com".to_string();
        mock_repo.expect_create().returning(|_| Ok(Uuid::new_v4()));
        mock_repo.expect_count_by_email().returning(|_| Ok(0));
        mock_repo.expect_find_by_id().returning(|id| {
            Ok(Some(User {
                id,
                email: "test@example.com".into(),
                password_hash: "hash".into(),
                role: crate::domain::UserRole::User,
                first_name: None,
                last_name: None,
                vpc_ipv6_prefix: None,
            }))
        });

        let nats_url =
            std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
        let nats_client = async_nats::connect(nats_url).await.unwrap();
        let nats = crate::nats::TypedNatsClient::new(nats_client);

        let state = AppState {
            ctx: crate::application::ApiContext::default(),
            user_repo: Arc::new(mock_repo),
            app_repo: Arc::new(MockAppRepository::new()),
            scheduler: Arc::new(crate::domain::MockScheduler::new()),
            nats,
            router_addr: "http://localhost:8080".to_string(),
            frontend_url: "http://localhost:3000".to_string(),
            api_db: sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://localhost/dummy")
                .unwrap(),
            jwt_secret: "secret".to_string(),
            master_key: "key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            workspace_events: tokio::sync::broadcast::channel(1).0,
            mesh_status:
                tokio::sync::watch::channel(crate::application::vms::MeshStatus::default()).0,
            acme_email: "admin@mikrom.spluca.org".to_string(),
            acme_staging: true,
            acme_check_interval: 3600,
            github_repo: Arc::new(MockGithubRepository::default()),
            volume_repo: Arc::new(MockVolumeRepository::new()),
            github_app_id: None,
            github_private_key: None,
            github_app_slug: None,
            github_webhook_url_base: None,
            active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
        };

        let payload = RegisterRequest {
            email,
            password: "password".into(),
            first_name: None,
            last_name: None,
        };

        let response = __register_impl(State(state), Json(payload)).await;
        assert!(response.is_ok());
    }

    #[tokio::test]
    async fn test_login_success() {
        let mut mock_repo = MockUserRepository::new();
        let email = "test@example.com".to_string();
        let password = "password";
        let password_hash = crate::crypto::hash_password(password).unwrap();

        mock_repo.expect_find_by_email().returning(move |e| {
            Ok(Some(User {
                id: Uuid::new_v4(),
                email: e.to_string(),
                password_hash: password_hash.clone(),
                role: crate::domain::UserRole::User,
                first_name: None,
                last_name: None,
                vpc_ipv6_prefix: None,
            }))
        });

        let nats_url =
            std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
        let nats_client = async_nats::connect(nats_url).await.unwrap();
        let nats = crate::nats::TypedNatsClient::new(nats_client);

        let state = AppState {
            ctx: crate::application::ApiContext::default(),
            user_repo: Arc::new(mock_repo),
            app_repo: Arc::new(MockAppRepository::new()),
            scheduler: Arc::new(crate::domain::MockScheduler::new()),
            nats,
            router_addr: "http://localhost:8080".to_string(),
            frontend_url: "http://localhost:3000".to_string(),
            api_db: sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://localhost/dummy")
                .unwrap(),
            jwt_secret: "secret".to_string(),
            master_key: "key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            workspace_events: tokio::sync::broadcast::channel(1).0,
            mesh_status:
                tokio::sync::watch::channel(crate::application::vms::MeshStatus::default()).0,
            acme_email: "admin@mikrom.spluca.org".to_string(),
            acme_staging: true,
            acme_check_interval: 3600,
            github_repo: Arc::new(MockGithubRepository::default()),
            volume_repo: Arc::new(MockVolumeRepository::new()),
            github_app_id: None,
            github_private_key: None,
            github_app_slug: None,
            github_webhook_url_base: None,
            active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
        };

        let payload = LoginRequest {
            email,
            password: password.into(),
        };

        let response = __login_impl(State(state), Json(payload)).await;
        assert!(response.is_ok());
    }
}
