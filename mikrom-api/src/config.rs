use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct ApiConfig {
    pub database_url: String,
    pub nats_url: String,

    #[serde(default = "default_jwt_secret")]
    pub jwt_secret: String,

    #[serde(default = "default_master_key")]
    pub master_key: String,

    #[serde(default = "default_api_port")]
    pub api_port: u16,

    #[serde(default = "default_router_addr")]
    pub router_addr: String,

    #[serde(default = "default_frontend_url")]
    pub frontend_url: String,

    #[serde(default = "default_use_tls")]
    pub use_tls: bool,

    #[serde(default = "default_acme_email")]
    pub acme_email: String,

    #[serde(default = "default_acme_staging")]
    pub acme_staging: bool,

    #[serde(default = "default_acme_check_interval")]
    pub acme_check_interval: u64,

    pub certs_dir: Option<String>,

    pub github_app_id: Option<String>,
    pub github_client_id: Option<String>,
    pub github_client_secret: Option<String>,
    pub github_private_key: Option<String>,
    pub github_app_slug: Option<String>,
    pub github_webhook_url_base: Option<String>,
}

fn default_acme_email() -> String {
    "admin@mikrom.spluca.org".to_string()
}

fn default_acme_staging() -> bool {
    true
}

fn default_acme_check_interval() -> u64 {
    3600 // 1 hour
}

fn default_jwt_secret() -> String {
    "secret".to_string()
}

fn default_master_key() -> String {
    "default-master-key-change-me-in-production".to_string()
}

fn default_api_port() -> u16 {
    5001
}

fn default_router_addr() -> String {
    "http://192.168.122.1:80".to_string()
}

fn default_frontend_url() -> String {
    "http://localhost:3000".to_string()
}

fn default_use_tls() -> bool {
    false
}

impl ApiConfig {
    pub fn load() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        envy::from_env::<Self>().map_err(anyhow::Error::from)
    }
}
