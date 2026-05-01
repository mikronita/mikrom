use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(default = "default_log_level")]
    #[allow(dead_code)]
    pub log_level: String,
    #[serde(default = "default_registry")]
    pub registry: String,
    #[serde(default = "default_buildpack_builder")]
    pub buildpack_builder: String,
    #[serde(default = "default_nats_url")]
    pub nats_url: String,
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

fn default_buildpack_builder() -> String {
    "paketobuildpacks/ubuntu-noble-builder".to_string()
}

impl Config {
    pub fn from_env() -> Result<Self, envy::Error> {
        dotenvy::dotenv().ok();
        envy::from_env()
    }
}
