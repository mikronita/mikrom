use crate::AppState;
use crate::application::auth::{AuthResult, AuthService, RegisterParams};
use crate::domain::User;
use crate::error::ApiResult;
use axum::{
    Json,
    extract::{Multipart, State},
    http::StatusCode,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
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
    pub captcha_id: String,
    pub captcha_answer: String,
}

#[derive(Debug, Serialize, rovo::schemars::JsonSchema)]
pub struct CaptchaResponse {
    pub captcha_id: String,
    pub captcha_image: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CaptchaToken {
    pub answer: String,
    pub expires_at: i64,
}

#[derive(Debug, Serialize, rovo::schemars::JsonSchema)]
pub struct AuthResponse {
    pub user: UserResponse,
    pub token: Option<String>,
    pub requires_2fa: bool,
}

impl From<AuthResult> for AuthResponse {
    fn from(result: AuthResult) -> Self {
        Self {
            user: result.user.into(),
            token: result.token,
            requires_2fa: result.requires_2fa,
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
    pub totp_enabled: bool,
    pub email_notifications: bool,
    pub marketing_emails: bool,
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
            totp_enabled: user.totp_enabled,
            email_notifications: user.email_notifications,
            marketing_emails: user.marketing_emails,
        }
    }
}

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
    pub code: Option<String>,
}

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct UpdateProfileRequest {
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email_notifications: Option<bool>,
    pub marketing_emails: Option<bool>,
}

fn avatar_storage_dir() -> PathBuf {
    PathBuf::from(std::env::var("MIKROM_DATA_DIR").unwrap_or_else(|_| "./data".to_string()))
        .join("v1/uploads/avatars")
}

fn public_avatar_url(filename: &str) -> String {
    format!("/uploads/avatars/{filename}")
}

const MAX_AVATAR_BYTES: u64 = 2 * 1024 * 1024;

#[rovo::rovo]
pub async fn get_captcha(State(state): State<AppState>) -> ApiResult<Json<CaptchaResponse>> {
    use rand::RngExt;
    let mut rng = rand::rng();
    let is_addition = rng.random_bool(0.5);
    let (challenge, answer) = if is_addition {
        let num1 = rng.random_range(1..10);
        let num2 = rng.random_range(1..10);
        (
            format!("{} + {} = ?", num1, num2),
            (num1 + num2).to_string(),
        )
    } else {
        let num1 = rng.random_range(5..15);
        let num2 = rng.random_range(1..=num1);
        (
            format!("{} - {} = ?", num1, num2),
            (num1 - num2).to_string(),
        )
    };

    let line_x1 = rng.random_range(5..40);
    let line_y1 = rng.random_range(5..45);
    let line_x2 = rng.random_range(110..145);
    let line_y2 = rng.random_range(5..45);

    let line2_x1 = rng.random_range(5..40);
    let line2_y1 = rng.random_range(5..45);
    let line2_x2 = rng.random_range(110..145);
    let line2_y2 = rng.random_range(5..45);

    let text_x = rng.random_range(20..35);
    let text_y = rng.random_range(28..38);
    let rotate_angle = rng.random_range(-8..=8);

    let svg = format!(
        r##"<svg width="150" height="50" viewBox="0 0 150 50" xmlns="http://www.w3.org/2000/svg">
            <rect width="100%" height="100%" fill="#f1f5f9" rx="6"/>
            <line x1="{}" y1="{}" x2="{}" y2="{}" stroke="#cbd5e1" stroke-width="2"/>
            <line x1="{}" y1="{}" x2="{}" y2="{}" stroke="#94a3b8" stroke-width="1.5"/>
            <text x="{}" y="{}" font-family="sans-serif" font-size="20" font-weight="bold" fill="#1e293b" transform="rotate({} {} {})">{}</text>
        </svg>"##,
        line_x1,
        line_y1,
        line_x2,
        line_y2,
        line2_x1,
        line2_y1,
        line2_x2,
        line2_y2,
        text_x,
        text_y,
        rotate_angle,
        text_x,
        text_y,
        challenge
    );

    let base64_image = format!("data:image/svg+xml;base64,{}", STANDARD.encode(svg));
    let expires_at = chrono::Utc::now().timestamp() + 300;

    let token = CaptchaToken { answer, expires_at };

    let token_str = serde_json::to_string(&token).map_err(|e| {
        crate::error::ApiError::Internal(format!("Failed to serialize captcha token: {}", e))
    })?;

    let captcha_id = crate::infrastructure::crypto::encrypt(&token_str, &state.master_key)?;

    Ok(Json(CaptchaResponse {
        captcha_id,
        captcha_image: base64_image,
    }))
}

#[rovo::rovo]
pub async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegisterRequest>,
) -> ApiResult<(StatusCode, Json<AuthResponse>)> {
    info!(email = %payload.email, "Registering new user");

    if payload.captcha_id.is_empty() {
        return Err(crate::error::ApiError::BadRequest(
            "Captcha ID is required".to_string(),
        ));
    }
    let decrypted_token_str =
        crate::infrastructure::crypto::decrypt(&payload.captcha_id, &state.master_key)?;
    let token: CaptchaToken = serde_json::from_str(&decrypted_token_str)
        .map_err(|_| crate::error::ApiError::BadRequest("Invalid captcha token".to_string()))?;

    let now = chrono::Utc::now().timestamp();
    if now > token.expires_at {
        return Err(crate::error::ApiError::BadRequest(
            "Captcha has expired".to_string(),
        ));
    }

    if payload.captcha_answer.trim().to_lowercase() != token.answer.trim().to_lowercase() {
        return Err(crate::error::ApiError::BadRequest(
            "Incorrect captcha answer".to_string(),
        ));
    }

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

    let result = AuthService::login(&state, payload.email, payload.password, payload.code).await?;

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
        payload.email_notifications,
        payload.marketing_emails,
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

    let user = AuthService::update_profile_by_auth(
        &state,
        &auth.user_id,
        None,
        None,
        avatar_url,
        None,
        None,
    )
    .await?;

    Ok(Json(user.into()))
}

pub async fn upload_avatar(
    auth: crate::AuthUser,
    State(state): State<AppState>,
    multipart: Multipart,
) -> ApiResult<Json<UserResponse>> {
    upload_avatar_impl(auth, State(state), multipart).await
}

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[rovo::rovo]
pub async fn change_password(
    auth: crate::AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<ChangePasswordRequest>,
) -> ApiResult<StatusCode> {
    AuthService::change_password(
        &state,
        &auth.user_id,
        payload.current_password,
        payload.new_password,
    )
    .await?;

    Ok(StatusCode::OK)
}

#[derive(Debug, Serialize, rovo::schemars::JsonSchema)]
pub struct TotpSetupResponse {
    pub secret: String,
    pub otpauth_url: String,
}

#[rovo::rovo]
pub async fn setup_totp(
    auth: crate::AuthUser,
    State(state): State<AppState>,
) -> ApiResult<Json<TotpSetupResponse>> {
    let result = AuthService::setup_totp(&state, &auth.user_id).await?;

    Ok(Json(TotpSetupResponse {
        secret: result.secret,
        otpauth_url: result.otpauth_url,
    }))
}

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct VerifyTotpRequest {
    pub code: String,
}

#[rovo::rovo]
pub async fn verify_totp(
    auth: crate::AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<VerifyTotpRequest>,
) -> ApiResult<StatusCode> {
    AuthService::verify_totp(&state, &auth.user_id, payload.code).await?;

    Ok(StatusCode::OK)
}

#[rovo::rovo]
pub async fn disable_totp(
    auth: crate::AuthUser,
    State(state): State<AppState>,
) -> ApiResult<StatusCode> {
    AuthService::disable_totp(&state, &auth.user_id).await?;

    Ok(StatusCode::OK)
}

#[rovo::rovo]
pub async fn delete_account(
    auth: crate::AuthUser,
    State(state): State<AppState>,
) -> ApiResult<StatusCode> {
    AuthService::delete_account(&state, &auth.user_id).await?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct CreateTokenRequest {
    pub name: String,
}

#[derive(Debug, Serialize, rovo::schemars::JsonSchema)]
pub struct CreatedTokenResponse {
    pub token: String,
    pub details: crate::domain::personal_access_token::PersonalAccessToken,
}

#[rovo::rovo]
pub async fn create_personal_access_token(
    auth: crate::AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<CreateTokenRequest>,
) -> ApiResult<(StatusCode, Json<CreatedTokenResponse>)> {
    if payload.name.trim().is_empty() {
        return Err(crate::error::ApiError::BadRequest(
            "Token name cannot be empty".to_string(),
        ));
    }

    let user_id = uuid::Uuid::parse_str(&auth.user_id)
        .map_err(|_| crate::error::ApiError::Auth("Invalid user ID".to_string()))?;

    use rand::distr::{Alphanumeric, SampleString};
    use rand::rng;

    let token_secret = Alphanumeric.sample_string(&mut rng(), 32);
    let full_token = format!("mikrom_pat_{}", token_secret);

    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(full_token.as_bytes());
    let token_hash = hex::encode(hasher.finalize());

    let token_last_four = if full_token.len() >= 4 {
        full_token[full_token.len() - 4..].to_string()
    } else {
        full_token.clone()
    };

    let token_id = uuid::Uuid::new_v4();

    let details = state
        .ctx
        .personal_access_token_repo
        .create(token_id, user_id, payload.name, token_hash, token_last_four)
        .await
        .map_err(|e| crate::error::ApiError::Internal(e.to_string()))?;

    Ok((
        StatusCode::CREATED,
        Json(CreatedTokenResponse {
            token: full_token,
            details,
        }),
    ))
}

#[rovo::rovo]
pub async fn list_personal_access_tokens(
    auth: crate::AuthUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<crate::domain::personal_access_token::PersonalAccessToken>>> {
    let user_id = uuid::Uuid::parse_str(&auth.user_id)
        .map_err(|_| crate::error::ApiError::Auth("Invalid user ID".to_string()))?;

    let tokens = state
        .ctx
        .personal_access_token_repo
        .list_by_user(user_id)
        .await
        .map_err(|e| crate::error::ApiError::Internal(e.to_string()))?;

    Ok(Json(tokens))
}

#[rovo::rovo]
pub async fn revoke_personal_access_token(
    auth: crate::AuthUser,
    State(state): State<AppState>,
    axum::extract::Path(token_id): axum::extract::Path<uuid::Uuid>,
) -> ApiResult<StatusCode> {
    let user_id = uuid::Uuid::parse_str(&auth.user_id)
        .map_err(|_| crate::error::ApiError::Auth("Invalid user ID".to_string()))?;

    let deleted = state
        .ctx
        .personal_access_token_repo
        .delete(token_id, user_id)
        .await
        .map_err(|e| crate::error::ApiError::Internal(e.to_string()))?;

    if !deleted {
        return Err(crate::error::ApiError::NotFound(
            "Token not found".to_string(),
        ));
    }

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::github::MockGithubRepository;
    use crate::domain::{
        MockAppRepository, MockDatabaseRepository, MockTenantRepository, MockUserRepository,
        MockVolumeRepository, Tenant, User,
    };
    use axum::http::StatusCode;
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
                totp_secret: None,
                totp_enabled: false,
                deleted_at: None,
                email_notifications: true,
                marketing_emails: false,
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

        let nats =
            crate::nats::TypedNatsClient::new_custom(Arc::new(crate::nats::MockNatsClient::new()));

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

        let token = CaptchaToken {
            answer: "8".to_string(),
            expires_at: chrono::Utc::now().timestamp() + 300,
        };
        let token_str = serde_json::to_string(&token).unwrap();
        let captcha_id =
            crate::infrastructure::crypto::encrypt(&token_str, &state.master_key).unwrap();

        let payload = RegisterRequest {
            email,
            password: "password".into(),
            first_name: None,
            last_name: None,
            captcha_id,
            captcha_answer: "8".to_string(),
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
                totp_secret: None,
                totp_enabled: false,
                deleted_at: None,
                email_notifications: true,
                marketing_emails: false,
            }))
        });

        let nats =
            crate::nats::TypedNatsClient::new_custom(Arc::new(crate::nats::MockNatsClient::new()));

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
            code: None,
        };

        let response = __login_impl(State(state), Json(payload)).await;
        assert!(response.is_ok());
    }

    #[tokio::test]
    async fn test_login_requires_2fa() {
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
                totp_secret: Some("JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP".to_string()),
                totp_enabled: true,
                deleted_at: None,
                email_notifications: true,
                marketing_emails: false,
            }))
        });

        let nats =
            crate::nats::TypedNatsClient::new_custom(Arc::new(crate::nats::MockNatsClient::new()));

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
            code: None,
        };

        let response = __login_impl(State(state), Json(payload)).await;
        assert!(response.is_ok());
        let res = response.unwrap().0;
        assert!(res.requires_2fa);
        assert!(res.token.is_none());
    }

    #[tokio::test]
    async fn test_login_with_invalid_2fa() {
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
                totp_secret: Some("JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP".to_string()),
                totp_enabled: true,
                deleted_at: None,
                email_notifications: true,
                marketing_emails: false,
            }))
        });

        let nats =
            crate::nats::TypedNatsClient::new_custom(Arc::new(crate::nats::MockNatsClient::new()));

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
            code: Some("000000".into()),
        };

        let response = __login_impl(State(state), Json(payload)).await;
        assert!(response.is_err());
    }

    #[tokio::test]
    async fn test_login_with_valid_2fa() {
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
                totp_secret: Some("JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP".to_string()),
                totp_enabled: true,
                deleted_at: None,
                email_notifications: true,
                marketing_emails: false,
            }))
        });

        let nats =
            crate::nats::TypedNatsClient::new_custom(Arc::new(crate::nats::MockNatsClient::new()));

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

        // Generate correct current OTP
        use totp_rs::{Secret, TOTP};
        let totp = TOTP::new(
            totp_rs::Algorithm::SHA1,
            6,
            1,
            30,
            Secret::Encoded("JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP".to_string())
                .to_bytes()
                .unwrap(),
            None,
            String::new(),
        )
        .unwrap();
        let valid_code = totp.generate_current().unwrap();

        let payload = LoginRequest {
            email,
            password: password.into(),
            code: Some(valid_code),
        };

        let response = __login_impl(State(state), Json(payload)).await;
        assert!(response.is_ok());
        let res = response.unwrap().0;
        assert!(!res.requires_2fa);
        assert!(res.token.is_some());
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
                totp_secret: None,
                totp_enabled: false,
                deleted_at: None,
                email_notifications: true,
                marketing_emails: false,
            }))
        });
        mock_repo.expect_update_profile().returning(
            |id, first_name, last_name, avatar_url, email_notifications, marketing_emails| {
                assert!(first_name.is_none());
                assert!(last_name.is_none());
                assert!(email_notifications.is_none());
                assert!(marketing_emails.is_none());
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
                    totp_secret: None,
                    totp_enabled: false,
                    deleted_at: None,
                    email_notifications: true,
                    marketing_emails: false,
                })
            },
        );

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
            None,
            None,
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

    #[tokio::test]
    async fn test_change_password_success() {
        let user_id = Uuid::new_v4();
        let password_hash = crate::crypto::hash_password("current-password").unwrap();

        let mut mock_repo = MockUserRepository::new();
        mock_repo.expect_find_by_id().returning(move |id| {
            Ok(Some(User {
                id,
                email: "test@example.com".into(),
                password_hash: password_hash.clone(),
                role: crate::domain::UserRole::User,
                first_name: None,
                last_name: None,
                avatar_url: None,
                vpc_ipv6_prefix: None,
                totp_secret: None,
                totp_enabled: false,
                deleted_at: None,
                email_notifications: true,
                marketing_emails: false,
            }))
        });
        mock_repo.expect_update_password().returning(|_, _| Ok(()));

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

        let auth = crate::AuthUser {
            user_id: user_id.to_string(),
            email: "test@example.com".to_string(),
            role: crate::domain::UserRole::User,
        };

        let payload = ChangePasswordRequest {
            current_password: "current-password".to_string(),
            new_password: "new-password-123".to_string(),
        };

        let response = __change_password_impl(auth, State(state), Json(payload)).await;
        assert!(response.is_ok());
    }

    #[tokio::test]
    async fn test_change_password_wrong_current() {
        let user_id = Uuid::new_v4();
        let password_hash = crate::crypto::hash_password("actual-password").unwrap();

        let mut mock_repo = MockUserRepository::new();
        let captured_id = user_id;
        mock_repo.expect_find_by_id().returning(move |id| {
            Ok(Some(User {
                id,
                email: "test@example.com".into(),
                password_hash: password_hash.clone(),
                role: crate::domain::UserRole::User,
                first_name: None,
                last_name: None,
                avatar_url: None,
                vpc_ipv6_prefix: None,
                totp_secret: None,
                totp_enabled: false,
                deleted_at: None,
                email_notifications: true,
                marketing_emails: false,
            }))
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

        let auth = crate::AuthUser {
            user_id: captured_id.to_string(),
            email: "test@example.com".to_string(),
            role: crate::domain::UserRole::User,
        };

        let payload = ChangePasswordRequest {
            current_password: "wrong-password".to_string(),
            new_password: "new-password-123".to_string(),
        };

        let response = __change_password_impl(auth, State(state), Json(payload)).await;
        assert!(response.is_err());
    }

    #[tokio::test]
    async fn test_setup_totp_success() {
        let user_id = Uuid::new_v4();

        let mut mock_repo = MockUserRepository::new();
        mock_repo.expect_find_by_id().returning(move |id| {
            Ok(Some(User {
                id,
                email: "test@example.com".into(),
                password_hash: "hash".into(),
                role: crate::domain::UserRole::User,
                first_name: None,
                last_name: None,
                avatar_url: None,
                vpc_ipv6_prefix: None,
                totp_secret: None,
                totp_enabled: false,
                deleted_at: None,
                email_notifications: true,
                marketing_emails: false,
            }))
        });
        mock_repo
            .expect_update_totp_secret()
            .returning(|_, _| Ok(()));

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

        let auth = crate::AuthUser {
            user_id: user_id.to_string(),
            email: "test@example.com".to_string(),
            role: crate::domain::UserRole::User,
        };

        let response = __setup_totp_impl(auth, State(state)).await;
        assert!(response.is_ok());
        let result = response.unwrap();
        assert!(!result.secret.is_empty());
        assert!(result.otpauth_url.starts_with("otpauth://"));
    }

    #[tokio::test]
    async fn test_disable_totp_success() {
        let user_id = Uuid::new_v4();

        let mut mock_repo = MockUserRepository::new();
        mock_repo.expect_disable_totp().returning(|_| Ok(()));

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

        let auth = crate::AuthUser {
            user_id: user_id.to_string(),
            email: "test@example.com".to_string(),
            role: crate::domain::UserRole::User,
        };

        let response = __disable_totp_impl(auth, State(state)).await;
        assert!(response.is_ok());
    }

    #[tokio::test]
    async fn test_delete_account_success() {
        let user_id = Uuid::new_v4();

        let mut mock_repo = MockUserRepository::new();
        mock_repo.expect_soft_delete().returning(|_| Ok(()));

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

        let auth = crate::AuthUser {
            user_id: user_id.to_string(),
            email: "test@example.com".to_string(),
            role: crate::domain::UserRole::User,
        };

        let response = __delete_account_impl(auth, State(state)).await;
        assert!(response.is_ok());
        assert_eq!(response.unwrap(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_create_list_revoke_personal_access_tokens() {
        let user_id = Uuid::new_v4();

        let mut mock_pat_repo = crate::domain::MockPersonalAccessTokenRepository::new();

        let test_token = crate::domain::personal_access_token::PersonalAccessToken {
            id: Uuid::new_v4(),
            user_id,
            name: "test-token".to_string(),
            token_last_four: "abcd".to_string(),
            created_at: chrono::Utc::now(),
            last_used_at: None,
        };
        let _create_token_details = test_token.clone();
        mock_pat_repo
            .expect_create()
            .returning(move |_, _, name, _, _| {
                Ok(crate::domain::personal_access_token::PersonalAccessToken {
                    id: Uuid::new_v4(),
                    user_id,
                    name,
                    token_last_four: "abcd".to_string(),
                    created_at: chrono::Utc::now(),
                    last_used_at: None,
                })
            });

        let list_tokens = vec![test_token.clone()];
        mock_pat_repo
            .expect_list_by_user()
            .returning(move |_| Ok(list_tokens.clone()));

        mock_pat_repo.expect_delete().returning(|_, _| Ok(true));

        let ctx = crate::application::ApiContext {
            personal_access_token_repo: Arc::new(mock_pat_repo),
            ..Default::default()
        };

        let state = AppState {
            ctx: ctx.clone(),
            user_repo: Arc::new(MockUserRepository::new()),
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

        let auth = crate::AuthUser {
            user_id: user_id.to_string(),
            email: "test@example.com".to_string(),
            role: crate::domain::UserRole::User,
        };

        let payload = CreateTokenRequest {
            name: "test-token".to_string(),
        };
        let response =
            __create_personal_access_token_impl(auth.clone(), State(state.clone()), Json(payload))
                .await;
        assert!(response.is_ok());
        let (status, Json(created)) = response.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        assert!(created.token.starts_with("mikrom_pat_"));
        assert_eq!(created.details.name, "test-token");

        let response = __list_personal_access_tokens_impl(auth.clone(), State(state.clone())).await;
        assert!(response.is_ok());
        let Json(list) = response.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "test-token");

        let response = __revoke_personal_access_token_impl(
            auth,
            State(state),
            axum::extract::Path(test_token.id),
        )
        .await;
        assert!(response.is_ok());
        assert_eq!(response.unwrap(), StatusCode::NO_CONTENT);
    }
}
