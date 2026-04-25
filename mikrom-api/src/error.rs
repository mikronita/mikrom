use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: String,
    pub status: u16,
}

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Repository error: {0}")]
    Repo(#[from] crate::repositories::user_repository::DbError),

    #[error("Authentication failed: {0}")]
    Auth(String),

    #[error("Invalid token")]
    InvalidToken,

    #[error("Forbidden")]
    Forbidden,

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Internal server error: {0}")]
    Internal(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Scheduler error: {0}")]
    Scheduler(String),

    #[error("Anyhow error: {0}")]
    Anyhow(#[from] anyhow::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Database(err) => {
                tracing::error!("Database error: {:?}", err);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Database failure".to_string(),
                )
            },
            Self::Repo(err) => match err {
                crate::repositories::user_repository::DbError::NotFound => {
                    (StatusCode::NOT_FOUND, "Entity not found".to_string())
                },
                crate::repositories::user_repository::DbError::Conflict(msg) => {
                    (StatusCode::CONFLICT, msg)
                },
                crate::repositories::user_repository::DbError::Sqlx(e) => {
                    tracing::error!("Repository SQL error: {:?}", e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Database failure".to_string(),
                    )
                },
                crate::repositories::user_repository::DbError::Internal(msg) => {
                    tracing::error!("Repository internal error: {}", msg);
                    (StatusCode::INTERNAL_SERVER_ERROR, msg)
                },
            },
            Self::Auth(msg) => (StatusCode::UNAUTHORIZED, msg),
            Self::InvalidToken => (
                StatusCode::UNAUTHORIZED,
                "Invalid authentication token".to_string(),
            ),
            Self::Forbidden => (StatusCode::FORBIDDEN, "Forbidden access".to_string()),
            Self::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            Self::Conflict(msg) => (StatusCode::CONFLICT, msg),
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            Self::Scheduler(msg) => {
                tracing::error!("Scheduler communication error: {}", msg);
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Error communicating with scheduler".to_string(),
                )
            },
            Self::Anyhow(err) => {
                let msg = err.to_string();
                if msg.contains("is already taken") {
                    (StatusCode::CONFLICT, msg)
                } else {
                    tracing::error!("Anyhow error: {:?}", err);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Internal error: {}", err),
                    )
                }
            },
            Self::Internal(msg) => {
                tracing::error!("Internal error: {}", msg);
                (StatusCode::INTERNAL_SERVER_ERROR, msg)
            },
        };

        let body = Json(ErrorResponse {
            error: message,
            status: status.as_u16(),
        });

        (status, body).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
