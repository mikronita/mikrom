use crate::AppState;
use crate::application::auth::{AuthResult, AuthService, RegisterParams};
use crate::domain::User;
use crate::error::ApiResult;
use axum::{
    Json,
    extract::{Multipart, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use tracing::info;
use uuid::Uuid;

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
    pub avatar_url: Option<String>,
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
            avatar_url: user.avatar_url,
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

fn avatar_storage_dir() -> PathBuf {
    PathBuf::from("./data/avatars")
}

fn public_avatar_url(filename: &str) -> String {
    format!("/uploads/avatars/{filename}")
}

const MAX_AVATAR_BYTES: u64 = 2 * 1024 * 1024;

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
            avatar_url: None,
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
        None,
    )
    .await?;

    Ok(Json(user.into()))
}

pub async fn upload_avatar_impl(
    auth: crate::AuthUser,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> ApiResult<Json<UserResponse>> {
    let mut avatar_url = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| crate::error::ApiError::BadRequest(e.to_string()))?
    {
        if field.name() != Some("avatar") {
            continue;
        }

        let content_type = field.content_type().unwrap_or("application/octet-stream");
        let extension = match content_type {
            "image/png" => "png",
            "image/jpeg" | "image/jpg" => "jpg",
            "image/webp" => "webp",
            _ => {
                return Err(crate::error::ApiError::BadRequest(
                    "Unsupported avatar image type".into(),
                ));
            },
        };

        let bytes = field
            .bytes()
            .await
            .map_err(|e| crate::error::ApiError::BadRequest(e.to_string()))?;
        if bytes.len() as u64 > MAX_AVATAR_BYTES {
            return Err(crate::error::ApiError::BadRequest(
                "Avatar image is too large".into(),
            ));
        }
        let dir = avatar_storage_dir();
        fs::create_dir_all(&dir).map_err(|e| crate::error::ApiError::Internal(e.to_string()))?;

        let filename = format!("{}.{extension}", Uuid::new_v4());
        let path = dir.join(&filename);
        fs::write(&path, bytes).map_err(|e| crate::error::ApiError::Internal(e.to_string()))?;
        avatar_url = Some(public_avatar_url(&filename));
        break;
    }

    if avatar_url.is_none() {
        return Err(crate::error::ApiError::BadRequest(
            "Missing avatar file field".into(),
        ));
    }

    let user =
        AuthService::update_profile_by_auth(&state, &auth.user_id, None, None, avatar_url).await?;

    Ok(Json(user.into()))
}

pub async fn upload_avatar(
    auth: crate::AuthUser,
    State(state): State<AppState>,
    multipart: Multipart,
) -> ApiResult<Json<UserResponse>> {
    upload_avatar_impl(auth, State(state), multipart).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::github::MockGithubRepository;
    use crate::domain::{
        MockAppRepository, MockDatabaseRepository, MockTenantRepository, MockUserRepository,
        MockVolumeRepository, Tenant, User,
    };
    use std::sync::Arc;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_register_success() {
        let mut mock_repo = MockUserRepository::new();
        let mut mock_tenant_repo = MockTenantRepository::new();
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
                avatar_url: None,
                vpc_ipv6_prefix: None,
            }))
        });
        mock_tenant_repo.expect_create().returning(|name, slug| {
            Ok(Tenant {
                id: Uuid::new_v4(),
                tenant_id: slug,
                name,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            })
        });
        mock_tenant_repo
            .expect_add_member()
            .returning(|_, _, _| Ok(()));

        let nats = crate::nats::TypedNatsClient::new_custom(Arc::new(
            crate::nats::MockNatsClient::new(),
        ));

        let state = AppState {
            ctx: crate::application::ApiContext::default(),
            user_repo: Arc::new(mock_repo),
            tenant_repo: Arc::new(mock_tenant_repo),
            app_repo: Arc::new(MockAppRepository::new()),
            database_repo: Arc::new(MockDatabaseRepository::new()),
            github_repo: Arc::new(MockGithubRepository::default()),
            volume_repo: Arc::new(MockVolumeRepository::new()),
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
                avatar_url: None,
                vpc_ipv6_prefix: None,
            }))
        });

        let nats = crate::nats::TypedNatsClient::new_custom(Arc::new(
            crate::nats::MockNatsClient::new(),
        ));

        let state = AppState {
            ctx: crate::application::ApiContext::default(),
            user_repo: Arc::new(mock_repo),
            tenant_repo: Arc::new(crate::domain::MockTenantRepository::new()),
            app_repo: Arc::new(MockAppRepository::new()),
            database_repo: Arc::new(MockDatabaseRepository::new()),
            github_repo: Arc::new(MockGithubRepository::default()),
            volume_repo: Arc::new(MockVolumeRepository::new()),
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

    #[tokio::test]
    async fn test_upload_avatar_saves_png_and_updates_profile() {
        let mut mock_repo = MockUserRepository::new();
        mock_repo.expect_find_by_id().returning(|id| {
            Ok(Some(User {
                id,
                email: "test@example.com".into(),
                password_hash: "hash".into(),
                role: crate::domain::UserRole::User,
                first_name: None,
                last_name: None,
                avatar_url: Some("/uploads/avatars/test.png".into()),
                vpc_ipv6_prefix: None,
            }))
        });
        mock_repo
            .expect_update_profile()
            .returning(|id, first_name, last_name, avatar_url| {
                assert!(first_name.is_none());
                assert!(last_name.is_none());
                let url = avatar_url.expect("expected avatar url");
                assert!(url.starts_with("/uploads/avatars/"));
                Ok(User {
                    id,
                    email: "test@example.com".into(),
                    password_hash: "hash".into(),
                    role: crate::domain::UserRole::User,
                    first_name: None,
                    last_name: None,
                    avatar_url: None,
                    vpc_ipv6_prefix: None,
                })
            });

        let state = AppState {
            ctx: crate::application::ApiContext::default(),
            user_repo: Arc::new(mock_repo),
            tenant_repo: Arc::new(MockTenantRepository::new()),
            app_repo: Arc::new(MockAppRepository::new()),
            database_repo: Arc::new(MockDatabaseRepository::new()),
            github_repo: Arc::new(MockGithubRepository::default()),
            volume_repo: Arc::new(MockVolumeRepository::new()),
            scheduler: Arc::new(crate::domain::MockScheduler::new()),
            nats: crate::nats::TypedNatsClient::new_custom(Arc::new(
                crate::nats::MockNatsClient::new(),
            )),
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
            github_app_id: None,
            github_private_key: None,
            github_app_slug: None,
            github_webhook_url_base: None,
            active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
        };

        let filename = format!("{}.png", Uuid::new_v4());
        let dir = std::path::Path::new("./data/avatars");
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(&filename), b"png-bytes").unwrap();

        let response = AuthService::update_profile_by_auth(
            &state,
            &Uuid::new_v4().to_string(),
            None,
            None,
            Some(format!("/uploads/avatars/{filename}")),
        )
        .await
        .unwrap();
        assert!(response.avatar_url.is_some());
        assert!(
            std::fs::read_dir("./data/avatars")
                .unwrap()
                .next()
                .is_some()
        );
    }
}
