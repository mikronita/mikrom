use crate::AppState;
use crate::error::{ApiError, ApiResult};
use crate::repositories::user_repository::NewUser;
use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::info;
use utoipa::ToSchema;

#[derive(Debug, Deserialize, ToSchema)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuthResponse {
    pub user: UserResponse,
    pub token: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UserResponse {
    pub id: String,
    pub email: String,
    pub role: crate::repositories::user_repository::UserRole,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateProfileRequest {
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

#[utoipa::path(
    post,
    path = "/auth/register",
    request_body = RegisterRequest,
    responses(
        (status = 201, description = "User registered successfully", body = AuthResponse),
        (status = 400, description = "Bad request", body = crate::error::ErrorResponse),
        (status = 409, description = "User already exists", body = crate::error::ErrorResponse)
    ),
    tag = "auth"
)]
pub async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegisterRequest>,
) -> ApiResult<(StatusCode, Json<AuthResponse>)> {
    info!(email = %payload.email, "Registering new user");

    // Check if user already exists
    let count = state
        .user_repo
        .count_by_email(&payload.email)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    if count > 0 {
        return Err(ApiError::Conflict("User already exists".into()));
    }

    // Hash password
    let password_hash = crate::auth::crypto::hash_password(&payload.password)
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    // Create user
    let user_id = state
        .user_repo
        .create(NewUser {
            email: payload.email.clone(),
            password_hash,
            role: crate::repositories::user_repository::UserRole::User,
            first_name: payload.first_name.clone(),
            last_name: payload.last_name.clone(),
        })
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let user = state
        .user_repo
        .find_by_id(user_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or_else(|| ApiError::Internal("User not found after creation".into()))?;

    // Generate JWT
    let token = crate::auth::jwt::create_token(
        &user.id.to_string(),
        &user.email,
        &user.role,
        &state.jwt_secret,
    )
    .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok((
        StatusCode::CREATED,
        Json(AuthResponse {
            user: UserResponse {
                id: user.id.to_string(),
                email: user.email,
                role: user.role,
                first_name: user.first_name,
                last_name: user.last_name,
            },
            token,
        }),
    ))
}

#[utoipa::path(
    post,
    path = "/auth/login",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "User logged in successfully", body = AuthResponse),
        (status = 401, description = "Invalid credentials", body = crate::error::ErrorResponse)
    ),
    tag = "auth"
)]
pub async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> ApiResult<Json<AuthResponse>> {
    info!(email = %payload.email, "User login attempt");

    let user = state
        .user_repo
        .find_by_email(&payload.email)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or(ApiError::Auth("Invalid credentials".into()))?;

    // Verify password
    if !crate::auth::crypto::verify_password(&payload.password, &user.password_hash)
        .map_err(|_| ApiError::Auth("Invalid credentials".into()))?
    {
        return Err(ApiError::Auth("Invalid credentials".into()));
    }

    // Generate JWT
    let token = crate::auth::jwt::create_token(
        &user.id.to_string(),
        &user.email,
        &user.role,
        &state.jwt_secret,
    )
    .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(Json(AuthResponse {
        user: UserResponse {
            id: user.id.to_string(),
            email: user.email,
            role: user.role,
            first_name: user.first_name,
            last_name: user.last_name,
        },
        token,
    }))
}

#[utoipa::path(
    get,
    path = "/auth/me",
    responses(
        (status = 200, description = "Get current user profile", body = UserResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "auth",
    security(
        ("jwt" = [])
    )
)]
pub async fn get_profile(
    auth: crate::auth::AuthUser,
    State(state): State<AppState>,
) -> ApiResult<Json<UserResponse>> {
    let user_uuid = uuid::Uuid::parse_str(&auth.user_id)
        .map_err(|_| ApiError::Auth("Invalid user ID in token".into()))?;

    let user = state
        .user_repo
        .find_by_id(user_uuid)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or(ApiError::NotFound("User not found".into()))?;

    Ok(Json(UserResponse {
        id: user.id.to_string(),
        email: user.email,
        role: user.role,
        first_name: user.first_name,
        last_name: user.last_name,
    }))
}

#[utoipa::path(
    put,
    path = "/auth/me",
    request_body = UpdateProfileRequest,
    responses(
        (status = 200, description = "Profile updated successfully", body = UserResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "auth",
    security(
        ("jwt" = [])
    )
)]
pub async fn update_profile(
    auth: crate::auth::AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<UpdateProfileRequest>,
) -> ApiResult<Json<UserResponse>> {
    let user_uuid = uuid::Uuid::parse_str(&auth.user_id)
        .map_err(|_| ApiError::Auth("Invalid user ID in token".into()))?;

    state
        .user_repo
        .update_profile(user_uuid, payload.first_name, payload.last_name)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let user = state
        .user_repo
        .find_by_id(user_uuid)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or(ApiError::NotFound("User not found".into()))?;

    Ok(Json(UserResponse {
        id: user.id.to_string(),
        email: user.email,
        role: user.role,
        first_name: user.first_name,
        last_name: user.last_name,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::app_repository::MockAppRepository;
    use crate::repositories::user_repository::{MockUserRepository, User};
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
                role: crate::repositories::user_repository::UserRole::User,
                first_name: None,
                last_name: None,
            }))
        });

        let nats_url =
            std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
        let nats_client = async_nats::connect(nats_url).await.unwrap();
        let nats = crate::nats::TypedNatsClient::new(nats_client);

        let state = AppState {
            user_repo: Arc::new(mock_repo),
            app_repo: Arc::new(MockAppRepository::new()),
            scheduler: Arc::new(crate::scheduler::MockScheduler::new()),
            nats,
            router_addr: "http://localhost:8080".to_string(),
            frontend_url: "http://localhost:3000".to_string(),
            api_db: sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://localhost/dummy")
                .unwrap(),
            jwt_secret: "secret".to_string(),
            master_key: "key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            acme_email: "admin@mikrom.spluca.org".to_string(),
            acme_staging: true,
            acme_check_interval: 3600,
            github_repo: Arc::new(crate::repositories::MockGithubRepository::default()),
            github_app_id: None,
            github_private_key: None,
            github_app_slug: None,
        };

        let payload = RegisterRequest {
            email,
            password: "password".into(),
            first_name: None,
            last_name: None,
        };

        let response = register(State(state), Json(payload)).await;
        assert!(response.is_ok());
    }

    #[tokio::test]
    async fn test_login_success() {
        let mut mock_repo = MockUserRepository::new();
        let email = "test@example.com".to_string();
        let password = "password";
        let password_hash = crate::auth::crypto::hash_password(password).unwrap();

        mock_repo.expect_find_by_email().returning(move |e| {
            Ok(Some(User {
                id: Uuid::new_v4(),
                email: e.to_string(),
                password_hash: password_hash.clone(),
                role: crate::repositories::user_repository::UserRole::User,
                first_name: None,
                last_name: None,
            }))
        });

        let nats_url =
            std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
        let nats_client = async_nats::connect(nats_url).await.unwrap();
        let nats = crate::nats::TypedNatsClient::new(nats_client);

        let state = AppState {
            user_repo: Arc::new(mock_repo),
            app_repo: Arc::new(MockAppRepository::new()),
            scheduler: Arc::new(crate::scheduler::MockScheduler::new()),
            nats,
            router_addr: "http://localhost:8080".to_string(),
            frontend_url: "http://localhost:3000".to_string(),
            api_db: sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://localhost/dummy")
                .unwrap(),
            jwt_secret: "secret".to_string(),
            master_key: "key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            acme_email: "admin@mikrom.spluca.org".to_string(),
            acme_staging: true,
            acme_check_interval: 3600,
            github_repo: Arc::new(crate::repositories::MockGithubRepository::default()),
            github_app_id: None,
            github_private_key: None,
            github_app_slug: None,
        };

        let payload = LoginRequest {
            email,
            password: password.into(),
        };

        let response = login(State(state), Json(payload)).await;
        assert!(response.is_ok());
    }
}
