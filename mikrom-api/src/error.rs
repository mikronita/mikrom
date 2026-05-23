use axum::{
    Json,
    http::StatusCode,
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
};
use futures::Stream;
use serde::Serialize;
use std::convert::Infallible;
use thiserror::Error;

#[derive(Serialize, serde::Deserialize, rovo::schemars::JsonSchema)]
pub struct ErrorResponse {
    pub error: String,
    pub status: u16,
}

impl rovo::aide::OperationOutput for ErrorResponse {
    type Inner = Self;

    fn operation_response(
        ctx: &mut rovo::aide::generate::GenContext,
        operation: &mut rovo::aide::openapi::Operation,
    ) -> Option<rovo::aide::openapi::Response> {
        <axum::Json<Self> as rovo::aide::OperationOutput>::operation_response(ctx, operation)
    }

    fn inferred_responses(
        ctx: &mut rovo::aide::generate::GenContext,
        operation: &mut rovo::aide::openapi::Operation,
    ) -> Vec<(Option<u16>, rovo::aide::openapi::Response)> {
        <axum::Json<Self> as rovo::aide::OperationOutput>::inferred_responses(ctx, operation)
    }
}

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Domain error: {0}")]
    Domain(#[from] crate::domain::DomainError),

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
            Self::Domain(err) => match err {
                crate::domain::DomainError::NotFound => {
                    (StatusCode::NOT_FOUND, "Entity not found".to_string())
                },
                crate::domain::DomainError::Conflict(msg) => (StatusCode::CONFLICT, msg),
                crate::domain::DomainError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg),
                crate::domain::DomainError::InvalidRequest(msg) => (StatusCode::BAD_REQUEST, msg),
                crate::domain::DomainError::Infrastructure(msg) => {
                    tracing::error!("Domain infrastructure error: {}", msg);
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
                tracing::error!("Scheduler error: {}", msg);
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    format!("Scheduler error: {}", msg),
                )
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

impl rovo::aide::OperationOutput for ApiError {
    type Inner = ErrorResponse;

    fn operation_response(
        ctx: &mut rovo::aide::generate::GenContext,
        operation: &mut rovo::aide::openapi::Operation,
    ) -> Option<rovo::aide::openapi::Response> {
        <axum::Json<ErrorResponse> as rovo::aide::OperationOutput>::operation_response(
            ctx, operation,
        )
    }

    fn inferred_responses(
        ctx: &mut rovo::aide::generate::GenContext,
        operation: &mut rovo::aide::openapi::Operation,
    ) -> Vec<(Option<u16>, rovo::aide::openapi::Response)> {
        if let Some(res) = Self::operation_response(ctx, operation) {
            vec![(None, res)]
        } else {
            vec![]
        }
    }
}

pub struct SseResponse<S>(pub Sse<S>);

impl<S> IntoResponse for SseResponse<S>
where
    S: Stream<Item = Result<Event, Infallible>> + Send + 'static,
{
    fn into_response(self) -> Response {
        self.0.into_response()
    }
}

impl<S> rovo::aide::OperationOutput for SseResponse<S>
where
    S: Stream<Item = Result<Event, Infallible>> + Send + 'static,
{
    type Inner = Self;

    fn operation_response(
        _ctx: &mut rovo::aide::generate::GenContext,
        _operation: &mut rovo::aide::openapi::Operation,
    ) -> Option<rovo::aide::openapi::Response> {
        let mut content = indexmap::IndexMap::new();
        content.insert(
            "text/event-stream".to_string(),
            rovo::aide::openapi::MediaType {
                schema: None,
                ..Default::default()
            },
        );

        Some(rovo::aide::openapi::Response {
            description: "Server-Sent Events stream".to_string(),
            content,
            ..Default::default()
        })
    }

    fn inferred_responses(
        ctx: &mut rovo::aide::generate::GenContext,
        operation: &mut rovo::aide::openapi::Operation,
    ) -> Vec<(Option<u16>, rovo::aide::openapi::Response)> {
        if let Some(res) = Self::operation_response(ctx, operation) {
            vec![(Some(200), res)]
        } else {
            vec![]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::StatusCode;

    #[tokio::test]
    async fn scheduler_error_is_exposed_in_response_body() {
        let response = ApiError::Scheduler("scheduler unavailable".to_string()).into_response();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let error: ErrorResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(error.error, "Scheduler error: scheduler unavailable");
        assert_eq!(error.status, StatusCode::SERVICE_UNAVAILABLE.as_u16());
    }
}
