use crate::domain::UserRole;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub email: String,
    pub role: UserRole,
    pub exp: u64,
    pub iat: u64,
}

/// Creates a new JWT token for a user.
///
/// # Errors
///
/// Returns an error if token encoding fails.
pub fn create_token(
    user_id: &str,
    email: &str,
    role: &UserRole,
    secret: &str,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = chrono::Utc::now().timestamp();
    let now_u64 = u64::try_from(now).unwrap_or(0);
    let expiration = now_u64 + 3600 * 24;

    let claims = Claims {
        sub: user_id.to_string(),
        email: email.to_string(),
        role: role.clone(),
        exp: expiration,
        iat: now_u64,
    };

    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )
}

/// Verifies a JWT token and returns its claims.
///
/// # Errors
///
/// Returns an error if the token is invalid or expired.
pub fn verify_token(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let token_data = jsonwebtoken::decode::<Claims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
        &jsonwebtoken::Validation::default(),
    )?;
    Ok(token_data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET: &str = "test-secret-key";

    #[test]
    fn test_create_token_success() {
        let result = create_token("user-123", "test@example.com", &UserRole::User, TEST_SECRET);
        assert!(result.is_ok());
        let token = result.unwrap();
        assert!(!token.is_empty());
        assert!(token.contains('.'));
    }

    #[test]
    fn test_verify_token_success() {
        let token = create_token(
            "user-456",
            "verify@example.com",
            &UserRole::User,
            TEST_SECRET,
        )
        .unwrap();
        let claims = verify_token(&token, TEST_SECRET).unwrap();

        assert_eq!(claims.sub, "user-456");
        assert_eq!(claims.email, "verify@example.com");
        assert_eq!(claims.role, UserRole::User);
    }

    #[test]
    fn test_verify_token_invalid_secret() {
        let token =
            create_token("user-789", "test@example.com", &UserRole::User, TEST_SECRET).unwrap();
        let result = verify_token(&token, "wrong-secret");
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_token_malformed_token() {
        let result = verify_token("not.a.valid.token", TEST_SECRET);
        assert!(result.is_err());
    }

    #[test]
    fn test_claims_deserialization() {
        let token =
            create_token("user-test", "claims@test.com", &UserRole::User, TEST_SECRET).unwrap();
        let claims = verify_token(&token, TEST_SECRET).unwrap();

        assert_eq!(claims.sub, "user-test");
        assert_eq!(claims.email, "claims@test.com");
        assert_eq!(claims.role, UserRole::User);
        assert!(claims.exp > claims.iat);
    }
}
