use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub email: String,
    pub exp: usize,
    pub iat: usize,
}

pub fn create_token(
    user_id: &str,
    email: &str,
    secret: &str,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = chrono::Utc::now().timestamp() as usize;
    let expiration = now + 3600 * 24;

    let claims = Claims {
        sub: user_id.to_string(),
        email: email.to_string(),
        exp: expiration,
        iat: now,
    };

    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )
}

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
        let result = create_token("user-123", "test@example.com", TEST_SECRET);
        assert!(result.is_ok());
        let token = result.unwrap();
        assert!(!token.is_empty());
        assert!(token.contains('.'));
    }

    #[test]
    fn test_verify_token_success() {
        let token = create_token("user-456", "verify@example.com", TEST_SECRET).unwrap();
        let claims = verify_token(&token, TEST_SECRET).unwrap();

        assert_eq!(claims.sub, "user-456");
        assert_eq!(claims.email, "verify@example.com");
    }

    #[test]
    fn test_verify_token_invalid_secret() {
        let token = create_token("user-789", "test@example.com", TEST_SECRET).unwrap();
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
        let token = create_token("user-test", "claims@test.com", TEST_SECRET).unwrap();
        let claims = verify_token(&token, TEST_SECRET).unwrap();

        assert_eq!(claims.sub, "user-test");
        assert_eq!(claims.email, "claims@test.com");
        assert!(claims.exp > claims.iat);
    }
}
