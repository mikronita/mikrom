use thiserror::Error;

/// Unified domain error type for all business-logic failures.
///
/// Replaces `anyhow::Result` in repository traits and application services so
/// callers can match on semantics instead of parsing opaque strings.
#[derive(Error, Debug, Clone, PartialEq)]
pub enum DomainError {
    #[error("Entity not found")]
    NotFound,

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Bad request: {0}")]
    InvalidRequest(String),

    #[error("Infrastructure error: {0}")]
    Infrastructure(String),
}

/// Convenience alias used throughout the domain and application layers.
pub type DomainResult<T> = Result<T, DomainError>;

impl From<sqlx::Error> for DomainError {
    fn from(e: sqlx::Error) -> Self {
        use sqlx::Error as SqlxError;
        match e {
            SqlxError::RowNotFound => Self::NotFound,
            SqlxError::Database(db_err) => {
                let code = db_err.code();
                if code.as_deref() == Some("23505") {
                    Self::Conflict(db_err.message().to_string())
                } else {
                    Self::Infrastructure(db_err.message().to_string())
                }
            },
            _ => Self::Infrastructure(e.to_string()),
        }
    }
}

impl From<anyhow::Error> for DomainError {
    fn from(e: anyhow::Error) -> Self {
        Self::Infrastructure(e.to_string())
    }
}

impl From<crate::error::ApiError> for DomainError {
    fn from(e: crate::error::ApiError) -> Self {
        Self::Infrastructure(e.to_string())
    }
}

impl From<uuid::Error> for DomainError {
    fn from(e: uuid::Error) -> Self {
        Self::InvalidRequest(format!("Invalid UUID: {}", e))
    }
}

impl From<serde_json::Error> for DomainError {
    fn from(e: serde_json::Error) -> Self {
        Self::Infrastructure(format!("JSON error: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_error_notfound_display() {
        assert_eq!(DomainError::NotFound.to_string(), "Entity not found");
    }

    #[test]
    fn domain_error_conflict_display() {
        assert_eq!(
            DomainError::Conflict("name taken".to_string()).to_string(),
            "Conflict: name taken"
        );
    }

    #[test]
    fn sqlx_row_notfound_maps_to_notfound() {
        let e: DomainError = sqlx::Error::RowNotFound.into();
        assert!(matches!(e, DomainError::NotFound));
    }
}
