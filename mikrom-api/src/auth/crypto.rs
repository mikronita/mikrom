use crate::error::ApiError;
use bcrypt::{DEFAULT_COST, hash, verify};

pub fn hash_password(password: &str) -> Result<String, ApiError> {
    hash(password, DEFAULT_COST)
        .map_err(|e| ApiError::Internal(format!("Password hashing failed: {}", e)))
}

pub fn verify_password(password: &str, hash: &str) -> Result<bool, ApiError> {
    verify(password, hash)
        .map_err(|e| ApiError::Auth(format!("Password verification failed: {}", e)))
}
