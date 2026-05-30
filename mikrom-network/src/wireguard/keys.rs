use crate::wireguard::error::NetworkError;
use async_trait::async_trait;
use base64::Engine as _;
use std::path::Path;
use tracing::info;
use zeroize::Zeroize;

#[async_trait]
pub trait WireGuardKeyStore: Sync + Send {
    async fn read_key(&self, path: &Path) -> Result<Option<String>, NetworkError>;
    async fn write_key(&self, path: &Path, key: &str) -> Result<(), NetworkError>;
}

pub struct FileWireGuardKeyStore;

#[async_trait]
impl WireGuardKeyStore for FileWireGuardKeyStore {
    async fn read_key(&self, path: &Path) -> Result<Option<String>, NetworkError> {
        if !path.exists() {
            return Ok(None);
        }

        let content = tokio::fs::read_to_string(path).await?;
        Ok(Some(content.trim().to_string()))
    }

    async fn write_key(&self, path: &Path, key: &str) -> Result<(), NetworkError> {
        tokio::fs::write(path, key).await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
        }
        Ok(())
    }
}

pub struct KeyManager;

impl KeyManager {
    pub async fn load_or_generate_key(
        data_dir: &str,
        store: &impl WireGuardKeyStore,
    ) -> Result<String, NetworkError> {
        let data_path = Path::new(data_dir);
        if !data_path.exists() {
            tokio::fs::create_dir_all(data_path).await?;
        }

        let key_path = data_path.join("wg.key");

        if let Some(key) = store.read_key(&key_path).await? {
            return Ok(key);
        }

        info!("Generating new WireGuard private key...");
        let secret = x25519_dalek::StaticSecret::random_from_rng(rand::thread_rng());
        let priv_bytes = secret.to_bytes();

        let priv_b64 = base64::engine::general_purpose::STANDARD.encode(priv_bytes);

        store.write_key(&key_path, &priv_b64).await?;

        Ok(priv_b64)
    }

    pub fn get_public_key(private_key: &str) -> Result<String, NetworkError> {
        let mut priv_bytes = base64::engine::general_purpose::STANDARD
            .decode(private_key.trim())
            .map_err(|e| NetworkError::KeyDecode(e.to_string()))?;

        let secret = x25519_dalek::StaticSecret::from(
            <[u8; 32]>::try_from(priv_bytes.as_slice())
                .map_err(|_| NetworkError::KeyDecode("Invalid key length".to_string()))?,
        );

        // Zeroize the decoded bytes after use
        priv_bytes.zeroize();

        let public = x25519_dalek::PublicKey::from(&secret);
        Ok(base64::engine::general_purpose::STANDARD.encode(public.as_bytes()))
    }

    pub fn decode_private_key(
        private_key: &str,
    ) -> Result<zeroize::Zeroizing<Vec<u8>>, NetworkError> {
        let key = base64::engine::general_purpose::STANDARD
            .decode(private_key.trim())
            .map_err(|e| NetworkError::KeyDecode(e.to_string()))?;

        if key.len() != 32 {
            return Err(NetworkError::KeyDecode(format!(
                "Invalid private key length: expected 32 bytes, got {}",
                key.len()
            )));
        }

        Ok(zeroize::Zeroizing::new(key))
    }

    pub fn normalize_public_key(public_key: &str) -> Result<Vec<u8>, NetworkError> {
        let normalized =
            if public_key.len() == 64 && public_key.chars().all(|c| c.is_ascii_hexdigit()) {
                hex::decode(public_key).map_err(|e| NetworkError::KeyDecode(e.to_string()))?
            } else {
                base64::engine::general_purpose::STANDARD
                    .decode(public_key.trim())
                    .map_err(|e| NetworkError::KeyDecode(e.to_string()))?
            };

        if normalized.len() != 32 {
            return Err(NetworkError::KeyDecode(format!(
                "Invalid public key length: expected 32 bytes, got {}",
                normalized.len()
            )));
        }

        Ok(normalized)
    }

    pub fn normalize_public_key_string(public_key: &str) -> Result<String, NetworkError> {
        let normalized = Self::normalize_public_key(public_key)?;
        Ok(base64::engine::general_purpose::STANDARD.encode(normalized))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_generate_and_load_key() {
        let temp_dir = tempfile::tempdir().unwrap();
        let data_dir = temp_dir.path().to_str().unwrap();
        let store = FileWireGuardKeyStore;

        let key1 = KeyManager::load_or_generate_key(data_dir, &store)
            .await
            .unwrap();
        let key2 = KeyManager::load_or_generate_key(data_dir, &store)
            .await
            .unwrap();

        assert_eq!(key1, key2);
        assert!(!key1.is_empty());
    }

    #[test]
    fn test_get_public_key() {
        // Known private key (32 bytes of 0x01)
        let priv_bytes = [1u8; 32];
        let priv_b64 = base64::engine::general_purpose::STANDARD.encode(priv_bytes);

        let pub_b64 = KeyManager::get_public_key(&priv_b64).unwrap();
        assert!(!pub_b64.is_empty());
    }
}
