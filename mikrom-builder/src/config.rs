use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(default = "default_log_level")]
    pub log_level: String,

    #[serde(default = "default_registry", rename = "REGISTRY_URL")]
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
        dotenvy::dotenv().ok();
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
    "localhost:5000".to_string()
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
