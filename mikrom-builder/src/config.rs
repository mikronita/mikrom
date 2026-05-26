use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(default = "default_log_level")]
    pub log_level: String,

    #[serde(default = "default_registry", alias = "registry_url")]
    pub registry: String,

    pub registry_user: Option<String>,
    pub registry_pass: Option<String>,

    #[serde(default = "default_max_concurrent_builds")]
    pub max_concurrent_builds: usize,

    #[serde(default = "default_build_state_ttl_secs")]
    pub build_state_ttl_secs: u64,

    #[serde(default = "default_build_state_path")]
    pub build_state_path: PathBuf,

    #[serde(default = "default_nats_url")]
    pub nats_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self, envy::Error> {
        load_env_files();
        envy::from_env()
    }
}

fn default_nats_url() -> String {
    std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string())
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_registry() -> String {
    "registry.mikrom.spluca.org/mikrom".to_string()
}

fn default_max_concurrent_builds() -> usize {
    2
}

fn default_build_state_ttl_secs() -> u64 {
    3600
}

fn default_build_state_path() -> PathBuf {
    PathBuf::from("/tmp/mikrom-builder-state.json")
}

fn load_env_files() {
    // Load the package .env as the base layer, then the working-directory .env as a
    // local override. Existing process environment variables keep precedence.
    let _ = dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/.env"));
    let _ = dotenvy::dotenv();
}

#[cfg(test)]
mod tests {
    use super::{Config, default_registry};
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn test_default_registry_matches_build_mode() {
        assert_eq!(default_registry(), "registry.mikrom.spluca.org/mikrom");
    }

    #[test]
    fn test_registry_prefers_registry_env_var_name() {
        let _guard = env_lock().lock().expect("env lock");

        let original_registry = std::env::var("REGISTRY").ok();
        let original_registry_url = std::env::var("REGISTRY_URL").ok();

        unsafe {
            std::env::set_var("REGISTRY", "registry.example.com/mikrom");
            std::env::remove_var("REGISTRY_URL");
        }

        let cfg = Config::from_env().expect("config should deserialize");

        match original_registry {
            Some(value) => unsafe { std::env::set_var("REGISTRY", value) },
            None => unsafe { std::env::remove_var("REGISTRY") },
        }

        match original_registry_url {
            Some(value) => unsafe { std::env::set_var("REGISTRY_URL", value) },
            None => unsafe { std::env::remove_var("REGISTRY_URL") },
        }

        assert_eq!(cfg.log_level, "info");
        assert_eq!(cfg.registry, "registry.example.com/mikrom");
        assert_eq!(cfg.max_concurrent_builds, 2);
        assert_eq!(cfg.build_state_ttl_secs, 3600);
        assert_eq!(
            cfg.build_state_path,
            std::path::PathBuf::from("/tmp/mikrom-builder-state.json")
        );
        assert_eq!(cfg.nats_url, "nats://localhost:4222");
    }
}
