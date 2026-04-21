use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub api_url: Option<String>,
    pub token: Option<String>,
}

impl Config {
    fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("mikrom").join("config.toml"))
    }

    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Self {
        let Ok(content) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        toml::from_str(&content).unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path =
            Self::path().ok_or_else(|| anyhow::anyhow!("Cannot determine config directory"))?;
        self.save_to(&path)
    }

    pub fn save_to(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn api_url(&self) -> &str {
        self.api_url.as_deref().unwrap_or("http://localhost:5001")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::get_unwrap)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_path(dir: &TempDir) -> PathBuf {
        dir.path().join("config.toml")
    }

    // ── api_url() ────────────────────────────────────────────────────────────────

    #[test]
    fn test_api_url_returns_default_when_none() {
        assert_eq!(Config::default().api_url(), "http://localhost:5001");
    }

    #[test]
    fn test_api_url_returns_custom_url() {
        let cfg = Config {
            api_url: Some("http://remote:9000".to_string()),
            token: None,
        };
        assert_eq!(cfg.api_url(), "http://remote:9000");
    }

    // ── load_from ────────────────────────────────────────────────────────────────

    #[test]
    fn test_load_from_missing_file_returns_default() {
        let dir = TempDir::new().unwrap();
        let cfg = Config::load_from(&dir.path().join("nonexistent.toml"));
        assert!(cfg.api_url.is_none());
        assert!(cfg.token.is_none());
    }

    #[test]
    fn test_load_from_invalid_toml_returns_default() {
        let dir = TempDir::new().unwrap();
        let path = temp_path(&dir);
        std::fs::write(&path, "not valid toml ][[[").unwrap();
        let cfg = Config::load_from(&path);
        assert!(cfg.api_url.is_none());
        assert!(cfg.token.is_none());
    }

    #[test]
    fn test_load_from_partial_only_api_url() {
        let dir = TempDir::new().unwrap();
        let path = temp_path(&dir);
        std::fs::write(&path, r#"api_url = "http://myserver:5001""#).unwrap();
        let cfg = Config::load_from(&path);
        assert_eq!(cfg.api_url.as_deref(), Some("http://myserver:5001"));
        assert!(cfg.token.is_none());
    }

    #[test]
    fn test_load_from_partial_only_token() {
        let dir = TempDir::new().unwrap();
        let path = temp_path(&dir);
        std::fs::write(&path, r#"token = "eyJhbGciOiJIUzI1NiJ9""#).unwrap();
        let cfg = Config::load_from(&path);
        assert!(cfg.api_url.is_none());
        assert_eq!(cfg.token.as_deref(), Some("eyJhbGciOiJIUzI1NiJ9"));
    }

    // ── save_to / roundtrip ──────────────────────────────────────────────────────

    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = temp_path(&dir);
        let original = Config {
            api_url: Some("http://example.com:5001".to_string()),
            token: Some("my-jwt-token".to_string()),
        };
        original.save_to(&path).unwrap();
        let loaded = Config::load_from(&path);
        assert_eq!(loaded.api_url, original.api_url);
        assert_eq!(loaded.token, original.token);
    }

    #[test]
    fn test_save_to_creates_parent_directories() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nested").join("dirs").join("config.toml");
        Config::default().save_to(&path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_save_to_overwrites_existing_file() {
        let dir = TempDir::new().unwrap();
        let path = temp_path(&dir);
        Config {
            api_url: Some("http://old:5001".to_string()),
            token: None,
        }
        .save_to(&path)
        .unwrap();
        Config {
            api_url: Some("http://new:5001".to_string()),
            token: Some("tok".to_string()),
        }
        .save_to(&path)
        .unwrap();
        let loaded = Config::load_from(&path);
        assert_eq!(loaded.api_url.as_deref(), Some("http://new:5001"));
        assert_eq!(loaded.token.as_deref(), Some("tok"));
    }

    #[test]
    fn test_load_from_empty_file_returns_default() {
        let dir = TempDir::new().unwrap();
        let path = temp_path(&dir);
        std::fs::write(&path, "").unwrap();
        let cfg = Config::load_from(&path);
        assert!(cfg.api_url.is_none());
        assert!(cfg.token.is_none());
    }

    #[test]
    fn test_load_from_comment_only_toml() {
        let dir = TempDir::new().unwrap();
        let path = temp_path(&dir);
        std::fs::write(&path, "# this is a comment\n").unwrap();
        let cfg = Config::load_from(&path);
        assert!(cfg.api_url.is_none());
        assert!(cfg.token.is_none());
    }

    #[test]
    fn test_load_from_full_config() {
        let dir = TempDir::new().unwrap();
        let path = temp_path(&dir);
        std::fs::write(
            &path,
            r#"
api_url = "http://full:9000"
token = "full-token-123"
"#,
        )
        .unwrap();
        let cfg = Config::load_from(&path);
        assert_eq!(cfg.api_url.as_deref(), Some("http://full:9000"));
        assert_eq!(cfg.token.as_deref(), Some("full-token-123"));
    }

    #[test]
    fn test_save_to_with_only_api_url() {
        let dir = TempDir::new().unwrap();
        let path = temp_path(&dir);
        Config {
            api_url: Some("http://api-only:8000".to_string()),
            token: None,
        }
        .save_to(&path)
        .unwrap();
        let loaded = Config::load_from(&path);
        assert_eq!(loaded.api_url.as_deref(), Some("http://api-only:8000"));
        assert!(loaded.token.is_none());
    }

    #[test]
    fn test_save_to_with_only_token() {
        let dir = TempDir::new().unwrap();
        let path = temp_path(&dir);
        Config {
            api_url: None,
            token: Some("token-only".to_string()),
        }
        .save_to(&path)
        .unwrap();
        let loaded = Config::load_from(&path);
        assert!(loaded.api_url.is_none());
        assert_eq!(loaded.token.as_deref(), Some("token-only"));
    }

    #[test]
    fn test_save_with_empty_config() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let cfg = Config::default();
        cfg.save_to(&path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_api_url_with_trailing_slash() {
        let cfg = Config {
            api_url: Some("http://example.com:5001/".to_string()),
            token: None,
        };
        assert_eq!(cfg.api_url(), "http://example.com:5001/");
    }

    #[test]
    fn test_api_url_with_path() {
        let cfg = Config {
            api_url: Some("http://example.com:5001/api/v1".to_string()),
            token: None,
        };
        assert_eq!(cfg.api_url(), "http://example.com:5001/api/v1");
    }

    #[test]
    fn test_default_values_are_none() {
        let cfg = Config::default();
        assert!(cfg.api_url.is_none());
        assert!(cfg.token.is_none());
    }

    #[test]
    fn test_config_serialization() {
        let cfg = Config {
            api_url: Some("http://test:5001".to_string()),
            token: Some("test-token".to_string()),
        };
        let serialized = toml::to_string(&cfg).unwrap();
        assert!(serialized.contains("api_url"));
        assert!(serialized.contains("token"));
    }

    #[test]
    fn test_config_deserialization() {
        let toml_str = r#"
api_url = "http://deser:5001"
token = "deser-token"
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.api_url.as_deref(), Some("http://deser:5001"));
        assert_eq!(cfg.token.as_deref(), Some("deser-token"));
    }
}
