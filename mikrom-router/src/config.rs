use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(rename = "router_http_port", default = "default_http_port")]
    pub http_port: u16,
    #[serde(rename = "router_https_port", default = "default_https_port")]
    pub https_port: u16,
    pub database_url: String,
    #[serde(default = "default_log_level")]
    #[allow(dead_code)]
    pub log_level: String,
    #[serde(default = "default_base_domain")]
    #[allow(dead_code)]
    pub base_domain: String,
    pub nats_url: String,
    pub acme_email: String,
    #[serde(default = "default_acme_staging")]
    pub acme_staging: bool,
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_http_port() -> u16 {
    8080
}

fn default_https_port() -> u16 {
    4343
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_base_domain() -> String {
    "apps.mikrom.es".to_string()
}

fn default_acme_staging() -> bool {
    true
}

impl Config {
    pub fn from_env() -> Result<Self, envy::Error> {
        dotenvy::dotenv().ok();
        envy::from_env()
    }
}
