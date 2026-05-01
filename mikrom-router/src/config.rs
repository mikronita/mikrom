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
    pub nats_url: String,
    pub master_key: String,
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl: u64,
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

fn default_cache_ttl() -> u64 {
    3600 // 1 hour
}

impl Config {
    pub fn from_env() -> Result<Self, envy::Error> {
        dotenvy::dotenv().ok();
        envy::from_env()
    }
}
