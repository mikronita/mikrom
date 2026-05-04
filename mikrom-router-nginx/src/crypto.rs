use aes_gcm::aead::AeadCore;
use aes_gcm::{AeadInPlace, Aes256Gcm, Key, KeyInit, Nonce, aead::OsRng};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("Encryption failed")]
    EncryptionFailed,
    #[error("Decryption failed")]
    DecryptionFailed,
    #[error("Invalid encoding")]
    InvalidEncoding,
    #[error("Invalid data length")]
    InvalidDataLength,
    #[error("UTF-8 error")]
    Utf8Error(#[from] std::string::FromUtf8Error),
}

pub fn encrypt(data: &str, master_key: &str) -> Result<String, CryptoError> {
    let key_bytes = hash_key(master_key);
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let mut buffer = data.as_bytes().to_vec();

    cipher
        .encrypt_in_place(&nonce, b"", &mut buffer)
        .map_err(|_| CryptoError::EncryptionFailed)?;

    // Combine nonce + ciphertext
    let mut result = nonce.to_vec();
    result.extend_from_slice(&buffer);

    Ok(STANDARD.encode(result))
}

pub fn decrypt(encrypted_data: &str, master_key: &str) -> Result<String, CryptoError> {
    let key_bytes = hash_key(master_key);
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    let combined = STANDARD
        .decode(encrypted_data)
        .map_err(|_| CryptoError::InvalidEncoding)?;

    if combined.len() < 12 {
        return Err(CryptoError::InvalidDataLength);
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    let mut buffer = ciphertext.to_vec();

    cipher
        .decrypt_in_place(nonce, b"", &mut buffer)
        .map_err(|_| CryptoError::DecryptionFailed)?;

    Ok(String::from_utf8(buffer)?)
}

fn hash_key(key: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hasher.finalize().into()
}
