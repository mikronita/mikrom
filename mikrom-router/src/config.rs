use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(rename = "router_port", default = "default_port")]
    pub port: u16,
    pub database_url: String,
    #[serde(default = "default_log_level")]
    #[allow(dead_code)]
    pub log_level: String,
    #[serde(default = "default_base_domain")]
    #[allow(dead_code)]
    pub base_domain: String,
    pub nats_url: String,
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    80
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_base_domain() -> String {
    "apps.mikrom.es".to_string()
}

impl Config {
    pub fn from_env() -> Result<Self, envy::Error> {
        dotenvy::dotenv().ok();
        envy::from_env()
    }
}
