use crate::AppState;
use crate::error::{ApiError, ApiResult};
use crate::repositories::user_repository::{NewUser, UserRole};
use axum::{Json, extract::State, http::StatusCode};
use bcrypt::{DEFAULT_COST, hash, verify};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Deserialize, ToSchema)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuthResponse {
    pub token: String,
    pub user_id: String,
    pub email: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize, ToSchema, Serialize)]
pub struct UpdateProfileRequest {
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UserProfileResponse {
    pub id: String,
    pub email: String,
    pub role: UserRole,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

#[utoipa::path(
    post,
    path = "/auth/register",
    request_body = RegisterRequest,
    responses(
        (status = 201, description = "User registered successfully"),
        (status = 400, description = "Invalid input", body = crate::error::ErrorResponse),
        (status = 409, description = "Email already exists", body = crate::error::ErrorResponse)
    ),
    tag = "auth"
)]
pub async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegisterRequest>,
) -> ApiResult<StatusCode> {
    let password_hash = hash(payload.password, DEFAULT_COST)
        .map_err(|_| ApiError::Internal("Hash failed".into()))?;
    state
        .user_repo
        .create(NewUser {
            email: payload.email,
            password_hash,
            role: UserRole::User,
            first_name: None,
            last_name: None,
        })
        .await?;
    Ok(StatusCode::CREATED)
}

#[utoipa::path(
    post,
    path = "/auth/login",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Login successful", body = AuthResponse),
        (status = 401, description = "Invalid credentials", body = crate::error::ErrorResponse)
    ),
    tag = "auth"
)]
pub async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> ApiResult<Json<AuthResponse>> {
    let user = state
        .user_repo
        .find_by_email(&payload.email)
        .await?
        .ok_or(ApiError::Auth("Invalid credentials".into()))?;

    if !verify(payload.password, &user.password_hash)
        .map_err(|_| ApiError::Internal("Verify failed".into()))?
    {
        return Err(ApiError::Auth("Invalid credentials".into()));
    }

    let token = crate::auth::jwt::create_token(
        &user.id.to_string(),
        &user.email,
        &user.role,
        &state.jwt_secret,
    )
    .map_err(|_| ApiError::Internal("Token generation failed".into()))?;

    Ok(Json(AuthResponse {
        token,
        user_id: user.id.to_string(),
        email: user.email,
    }))
}

#[utoipa::path(
    get,
    path = "/auth/profile",
    responses(
        (status = 200, description = "User profile", body = UserProfileResponse),
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
) -> ApiResult<Json<UserProfileResponse>> {
    let user_id = uuid::Uuid::parse_str(&auth.user_id)
        .map_err(|_| ApiError::BadRequest("Invalid user ID".into()))?;
    let user = state
        .user_repo
        .find_by_id(user_id)
        .await?
        .ok_or(ApiError::NotFound("User not found".into()))?;

    Ok(Json(UserProfileResponse {
        id: user.id.to_string(),
        email: user.email,
        role: user.role,
        first_name: user.first_name,
        last_name: user.last_name,
    }))
}

#[utoipa::path(
    patch,
    path = "/auth/profile",
    request_body = UpdateProfileRequest,
    responses(
        (status = 200, description = "Profile updated", body = UserProfileResponse),
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
) -> ApiResult<Json<UserProfileResponse>> {
    let user_id = uuid::Uuid::parse_str(&auth.user_id)
        .map_err(|_| ApiError::BadRequest("Invalid user ID".into()))?;
    state
        .user_repo
        .update_profile(user_id, payload.first_name, payload.last_name)
        .await?;

    let user = state
        .user_repo
        .find_by_id(user_id)
        .await?
        .ok_or(ApiError::NotFound("User not found".into()))?;

    Ok(Json(UserProfileResponse {
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
    use crate::repositories::user_repository::{MockUserRepository, User};
    use std::sync::Arc;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_register_success() {
        let mut mock_repo = MockUserRepository::new();
        let email = "test@example.com".to_string();
        mock_repo.expect_create().returning(|_| Ok(Uuid::new_v4()));

        let nats_url =
            std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
        let nats_client = async_nats::connect(nats_url).await.unwrap();
        let state = AppState {
            user_repo: Arc::new(mock_repo),
            app_repo: Arc::new(crate::repositories::app_repository::MockAppRepository::new()),
            scheduler: Arc::new(crate::scheduler::MockScheduler::new()),
            nats_client,
            router_addr: "http://localhost:8080".to_string(),
            api_db: sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://localhost/dummy")
                .unwrap(),
            jwt_secret: "secret".into(),
            master_key: "key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            acme_email: "admin@mikrom.es".into(),
            acme_staging: true,
            acme_check_interval: 3600,
        };

        let payload = RegisterRequest {
            email,
            password: "password".into(),
        };
        let result = register(State(state), Json(payload)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_login_success() {
        let mut mock_repo = MockUserRepository::new();
        let email = "test@example.com".to_string();
        let password = "password".to_string();
        let password_hash = hash(&password, DEFAULT_COST).unwrap();

        let user = User {
            id: Uuid::new_v4(),
            email: email.clone(),
            password_hash,
            role: UserRole::User,
            first_name: None,
            last_name: None,
        };

        mock_repo
            .expect_find_by_email()
            .returning(move |_| Ok(Some(user.clone())));

        let nats_url =
            std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
        let nats_client = async_nats::connect(nats_url).await.unwrap();
        let state = AppState {
            user_repo: Arc::new(mock_repo),
            app_repo: Arc::new(crate::repositories::app_repository::MockAppRepository::new()),
            scheduler: Arc::new(crate::scheduler::MockScheduler::new()),
            nats_client,
            router_addr: "http://localhost:8080".to_string(),
            api_db: sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://localhost/dummy")
                .unwrap(),
            jwt_secret: "secret".into(),
            master_key: "key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            acme_email: "admin@mikrom.es".into(),
            acme_staging: true,
            acme_check_interval: 3600,
        };

        let payload = LoginRequest { email, password };
        let result = login(State(state), Json(payload)).await;
        assert!(result.is_ok());
    }
}
