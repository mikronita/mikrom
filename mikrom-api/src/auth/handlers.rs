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
        .map_err(|e| ApiError::Internal(format!("Database error: {}", e.message)))?;

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
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to create user: {}", e.message)))?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(RegisterResponse {
            message: "User registered successfully".to_string(),
            user_id: user_id.to_string(),
        }),
    ))
}

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
        .map_err(|e| ApiError::Internal(format!("Database error: {}", e.message)))?
        .ok_or_else(|| ApiError::Auth("Invalid credentials".to_string()))?;

    if verify(&payload.password, &user.password_hash).unwrap_or(false) {
        let token =
            crate::auth::jwt::create_token(&user.id.to_string(), &user.email, &state.jwt_secret)
                .map_err(|_| ApiError::Internal("Failed to create token".to_string()))?;

        Ok(Json(LoginResponse { token }))
    } else {
        Err(ApiError::Auth("Invalid credentials".to_string()))
    }
}

// Tests ...
