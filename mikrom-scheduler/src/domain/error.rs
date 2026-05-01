use thiserror::Error;

/// Errors that can occur in the domain logic of the scheduler.
#[derive(Error, Debug)]
pub enum DomainError {
    #[error("No available workers")]
    NoWorkers,
    #[error("No worker can fit the VM requirements")]
    NoFit,
    #[error("Job not found: {0}")]
    JobNotFound(String),
    #[error("IP address pool exhausted")]
    IpPoolExhausted,
    #[error("Infrastructure error: {0}")]
    Infrastructure(String),
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
}

impl From<sqlx::Error> for DomainError {
    fn from(e: sqlx::Error) -> Self {
        Self::Infrastructure(e.to_string())
    }
}

pub type DomainResult<T> = Result<T, DomainError>;
