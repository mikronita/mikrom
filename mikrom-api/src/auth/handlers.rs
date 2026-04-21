use axum::{Json, extract::State};
use bcrypt::{DEFAULT_COST, hash, verify};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::error::{ApiError, ApiResult};
use crate::repositories::user_repository::NewUser;

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    pub message: String,
    pub user_id: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProfileRequest {
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ProfileResponse {
    pub id: String,
    pub email: String,
    pub role: crate::repositories::user_repository::UserRole,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

/// Registers a new user.
///
/// # Errors
///
/// Returns an error if the email is already registered or if validation fails.
#[tracing::instrument(skip(state, payload), fields(email = %payload.email))]
pub async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegisterRequest>,
) -> ApiResult<(axum::http::StatusCode, Json<RegisterResponse>)> {
    if payload.email.is_empty() || payload.password.is_empty() {
        return Err(ApiError::BadRequest(
            "Email and password are required".to_string(),
        ));
    }

    if payload.password.len() < 8 {
        return Err(ApiError::BadRequest(
            "Password must be at least 8 characters".to_string(),
        ));
    }

    let count = state
        .user_repo
        .count_by_email(&payload.email)
        .await
        .map_err(ApiError::from)?;

    if count > 0 {
        return Err(ApiError::Conflict("Email already registered".to_string()));
    }

    let password_hash = hash(&payload.password, DEFAULT_COST)
        .map_err(|_| ApiError::Internal("Failed to hash password".to_string()))?;

    let user_id = state
        .user_repo
        .create(NewUser {
            email: payload.email.clone(),
            password_hash,
            role: crate::repositories::user_repository::UserRole::User,
            first_name: None,
            last_name: None,
        })
        .await
        .map_err(ApiError::from)?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(RegisterResponse {
            message: "User registered successfully".to_string(),
            user_id: user_id.to_string(),
        }),
    ))
}

/// Logs in an existing user and returns a JWT token.
///
/// # Errors
///
/// Returns an error if credentials are invalid or token creation fails.
#[tracing::instrument(skip(state, payload), fields(email = %payload.email))]
pub async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> ApiResult<Json<LoginResponse>> {
    if payload.email.is_empty() || payload.password.is_empty() {
        return Err(ApiError::BadRequest(
            "Email and password are required".to_string(),
        ));
    }

    let user = state
        .user_repo
        .find_by_email(&payload.email)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::Auth("Invalid credentials".to_string()))?;

    if verify(&payload.password, &user.password_hash).unwrap_or(false) {
        let token = crate::auth::jwt::create_token(
            &user.id.to_string(),
            &user.email,
            &user.role,
            &state.jwt_secret,
        )
        .map_err(|_| ApiError::Internal("Failed to create token".to_string()))?;

        Ok(Json(LoginResponse { token }))
    } else {
        Err(ApiError::Auth("Invalid credentials".to_string()))
    }
}

/// Gets the profile of the currently authenticated user.
#[tracing::instrument(skip(state, auth))]
pub async fn get_profile(
    State(state): State<AppState>,
    auth: crate::auth::extractor::AuthUser,
) -> ApiResult<Json<ProfileResponse>> {
    let user_id = uuid::Uuid::parse_str(&auth.user_id)
        .map_err(|_| ApiError::Auth("Invalid user ID in token".to_string()))?;

    let user = state
        .user_repo
        .find_by_id(user_id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

    Ok(Json(ProfileResponse {
        id: user.id.to_string(),
        email: user.email,
        role: user.role,
        first_name: user.first_name,
        last_name: user.last_name,
    }))
}

/// Updates the profile of the currently authenticated user.
#[tracing::instrument(skip(state, auth, payload))]
pub async fn update_profile(
    State(state): State<AppState>,
    auth: crate::auth::extractor::AuthUser,
    Json(payload): Json<UpdateProfileRequest>,
) -> ApiResult<Json<ProfileResponse>> {
    let user_id = uuid::Uuid::parse_str(&auth.user_id)
        .map_err(|_| ApiError::Auth("Invalid user ID in token".to_string()))?;

    state
        .user_repo
        .update_profile(user_id, payload.first_name, payload.last_name)
        .await
        .map_err(ApiError::from)?;

    let user = state
        .user_repo
        .find_by_id(user_id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

    Ok(Json(ProfileResponse {
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
    use crate::repositories::user_repository::{MockUserRepository, User, UserRole};
    use std::sync::Arc;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_register_assigns_default_user_role() {
        let mut mock_repo = MockUserRepository::new();
        let email = "test@user.com".to_string();

        mock_repo.expect_count_by_email().returning(|_| Ok(0));
        mock_repo
            .expect_create()
            .withf(|u| u.role == UserRole::User)
            .returning(|_| Ok(Uuid::new_v4()));

        let state = crate::AppState {
            user_repo: Arc::new(mock_repo),
            scheduler_client: None,
            scheduler_config: crate::scheduler::SchedulerConfig::default(),
            jwt_secret: "secret".into(),
            master_key: "key".into(),
        };

        let payload = RegisterRequest {
            email,
            password: "password123".into(),
        };
        let result = register(State(state), Json(payload)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_login_includes_role_in_token() {
        let mut mock_repo = MockUserRepository::new();
        let email = "admin@mikrom.io".to_string();
        let password = "adminpassword".to_string();
        let hashed = hash(&password, DEFAULT_COST).unwrap();

        let user = User {
            id: Uuid::new_v4(),
            email: email.clone(),
            password_hash: hashed,
            role: UserRole::Admin,
            first_name: None,
            last_name: None,
        };

        mock_repo
            .expect_find_by_email()
            .returning(move |_| Ok(Some(user.clone())));

        let secret = "test-jwt-secret".to_string();
        let state = crate::AppState {
            user_repo: Arc::new(mock_repo),
            scheduler_client: None,
            scheduler_config: crate::scheduler::SchedulerConfig::default(),
            jwt_secret: secret.clone(),
            master_key: "key".into(),
        };

        let payload = LoginRequest { email, password };
        let result = login(State(state), Json(payload)).await.unwrap();

        let claims = crate::auth::jwt::verify_token(&result.token, &secret).unwrap();
        assert_eq!(claims.role, UserRole::Admin);
    }

    #[tokio::test]
    async fn test_get_profile_success() {
        let mut mock_repo = MockUserRepository::new();
        let user_id = Uuid::new_v4();
        let email = "profile@test.com".to_string();

        let user = User {
            id: user_id,
            email: email.clone(),
            password_hash: "hash".into(),
            role: UserRole::User,
            first_name: Some("Antonio".into()),
            last_name: Some("Pardo".into()),
        };

        mock_repo
            .expect_find_by_id()
            .with(mockall::predicate::eq(user_id))
            .returning(move |_| Ok(Some(user.clone())));

        let state = crate::AppState {
            user_repo: Arc::new(mock_repo),
            scheduler_client: None,
            scheduler_config: crate::scheduler::SchedulerConfig::default(),
            jwt_secret: "secret".into(),
            master_key: "key".into(),
        };

        let auth = crate::auth::extractor::AuthUser {
            user_id: user_id.to_string(),
            email,
            role: UserRole::User,
        };

        let result = get_profile(State(state), auth).await.unwrap();
        assert_eq!(result.id, user_id.to_string());
        assert_eq!(result.first_name, Some("Antonio".into()));
        assert_eq!(result.last_name, Some("Pardo".into()));
    }

    #[tokio::test]
    async fn test_update_profile_success() {
        let mut mock_repo = MockUserRepository::new();
        let user_id = Uuid::new_v4();
        let email = "update@test.com".to_string();

        mock_repo
            .expect_update_profile()
            .with(
                mockall::predicate::eq(user_id),
                mockall::predicate::eq(Some("New".into())),
                mockall::predicate::eq(Some("Name".into())),
            )
            .returning(|_, _, _| Ok(()));

        let updated_user = User {
            id: user_id,
            email: email.clone(),
            password_hash: "hash".into(),
            role: UserRole::User,
            first_name: Some("New".into()),
            last_name: Some("Name".into()),
        };

        mock_repo
            .expect_find_by_id()
            .returning(move |_| Ok(Some(updated_user.clone())));

        let state = crate::AppState {
            user_repo: Arc::new(mock_repo),
            scheduler_client: None,
            scheduler_config: crate::scheduler::SchedulerConfig::default(),
            jwt_secret: "secret".into(),
            master_key: "key".into(),
        };

        let auth = crate::auth::extractor::AuthUser {
            user_id: user_id.to_string(),
            email,
            role: UserRole::User,
        };

        let payload = UpdateProfileRequest {
            first_name: Some("New".into()),
            last_name: Some("Name".into()),
        };

        let result = update_profile(State(state), auth, Json(payload))
            .await
            .unwrap();
        assert_eq!(result.first_name, Some("New".into()));
        assert_eq!(result.last_name, Some("Name".into()));
    }
}
