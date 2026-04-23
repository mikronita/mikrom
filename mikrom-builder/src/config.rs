use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_log_level")]
    #[allow(dead_code)]
    pub log_level: String,
    #[serde(default = "default_registry")]
    pub registry: String,
    #[serde(default = "default_buildpack_builder")]
    pub buildpack_builder: String,
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    5004
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
