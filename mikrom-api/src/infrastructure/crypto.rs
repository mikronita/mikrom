use crate::error::ApiError;
use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rand::TryRng;
use rand::rngs::SysRng;

pub fn hash_password(password: &str) -> Result<String, ApiError> {
    let salt = bcrypt::DEFAULT_COST;
    bcrypt::hash(password, salt)
        .map_err(|e| ApiError::Internal(format!("Failed to hash password: {}", e)))
}

pub fn verify_password(password: &str, hash: &str) -> Result<bool, ApiError> {
    bcrypt::verify(password, hash)
        .map_err(|e| ApiError::Internal(format!("Failed to verify password: {}", e)))
}

pub fn encrypt(data: &str, master_key: &str) -> Result<String, ApiError> {
    let key_bytes = hash_key(master_key);
    #[allow(deprecated)]
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    let mut nonce_bytes = [0u8; 12];
    SysRng
        .try_fill_bytes(&mut nonce_bytes)
        .map_err(|e| ApiError::Internal(format!("Failed to generate nonce: {}", e)))?;
    #[allow(deprecated)]
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, data.as_bytes())
        .map_err(|_| ApiError::Internal("Encryption failed".to_string()))?;

    let mut result = nonce_bytes.to_vec();
    result.extend_from_slice(&ciphertext);

    Ok(STANDARD.encode(result))
}

pub fn decrypt(encrypted_data: &str, master_key: &str) -> Result<String, ApiError> {
    let key_bytes = hash_key(master_key);
    #[allow(deprecated)]
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    let combined = STANDARD
        .decode(encrypted_data)
        .map_err(|_| ApiError::BadRequest("Invalid base64 encoding".to_string()))?;

    if combined.len() < 12 + 16 {
        return Err(ApiError::BadRequest(
            "Invalid encrypted data length".to_string(),
        ));
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    #[allow(deprecated)]
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| ApiError::Auth("Decryption failed (check master key)".to_string()))?;

    String::from_utf8(plaintext)
        .map_err(|_| ApiError::Internal("Decrypted data is not valid UTF-8".to_string()))
}

fn hash_key(key: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let master = "my-super-secret-master-key";
        let secret = "db-password-123";

        let encrypted = encrypt(secret, master).unwrap();
        assert_ne!(encrypted, secret);

        let decrypted = decrypt(&encrypted, master).unwrap();
        assert_eq!(decrypted, secret);
    }

    #[test]
    fn test_decrypt_with_wrong_key_fails() {
        let secret = "hello";
        let encrypted = encrypt(secret, "key1").unwrap();
        let result = decrypt(&encrypted, "key2");
        assert!(result.is_err());
    }
}
