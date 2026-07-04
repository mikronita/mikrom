use aes_gcm::aead::AeadInPlace;
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("Invalid base64 encoding")]
    InvalidBase64,
    #[error("Invalid encrypted data length")]
    InvalidLength,
    #[error("Decryption failed (check master key)")]
    DecryptionFailed,
    #[error("Decrypted data is not valid UTF-8")]
    InvalidUtf8,
}

pub fn decrypt(encrypted_data: &str, master_key: &str) -> Result<String, CryptoError> {
    let key_bytes = hash_key(master_key);
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    let combined = STANDARD
        .decode(encrypted_data)
        .map_err(|_| CryptoError::InvalidBase64)?;

    if combined.len() < 12 + 16 {
        return Err(CryptoError::InvalidLength);
    }

    let (nonce_bytes, rest) = combined.split_at(12);
    let (ciphertext, tag_bytes) = rest.split_at(rest.len() - 16);

    let nonce = Nonce::from_slice(nonce_bytes);
    let tag = aes_gcm::aead::Tag::<Aes256Gcm>::from_slice(tag_bytes);
    let mut buffer = ciphertext.to_vec();

    cipher
        .decrypt_in_place_detached(nonce, b"", &mut buffer, tag)
        .map_err(|_| CryptoError::DecryptionFailed)?;

    String::from_utf8(buffer).map_err(|_| CryptoError::InvalidUtf8)
}

fn hash_key(key: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hasher.finalize().into()
}
