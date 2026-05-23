use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct ApiConfig {
    pub database_url: String,
    pub nats_url: String,

    pub jwt_secret: String,
    pub master_key: String,

    #[serde(default = "default_api_port")]
    pub api_port: u16,

    #[serde(default = "default_router_addr")]
    pub router_addr: String,

    #[serde(default = "default_frontend_url")]
    pub frontend_url: String,

    #[serde(default = "default_use_tls")]
    pub use_tls: bool,

    #[serde(default = "default_deployment_env")]
    pub deployment_env: String,

    pub rate_limit_public_rpm: Option<u32>,
    pub rate_limit_auth_login_rpm: Option<u32>,
    pub rate_limit_auth_register_rpm: Option<u32>,
    pub rate_limit_github_install_rpm: Option<u32>,
    pub rate_limit_apps_create_rpm: Option<u32>,
    pub rate_limit_apps_deploy_rpm: Option<u32>,
    pub rate_limit_webhooks_github_generic_rpm: Option<u32>,
    pub rate_limit_webhooks_github_named_rpm: Option<u32>,
    pub rate_limit_authenticated_read_rpm: Option<u32>,
    pub rate_limit_authenticated_write_rpm: Option<u32>,
    pub rate_limit_authenticated_stream_rpm: Option<u32>,

    #[serde(default = "default_rate_limit_entry_ttl_secs")]
    pub rate_limit_entry_ttl_secs: u64,

    #[serde(default = "default_rate_limit_cleanup_interval_secs")]
    pub rate_limit_cleanup_interval_secs: u64,

    #[serde(default = "default_rate_limit_trust_proxy_headers")]
    pub rate_limit_trust_proxy_headers: bool,

    #[serde(default = "default_nats_request_timeout_secs")]
    pub nats_request_timeout_secs: u64,

    #[serde(default = "default_nats_storage_timeout_secs")]
    pub nats_storage_timeout_secs: u64,

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

fn default_deployment_env() -> String {
    "development".to_string()
}

fn default_rate_limit_entry_ttl_secs() -> u64 {
    15 * 60
}

fn default_rate_limit_cleanup_interval_secs() -> u64 {
    60
}

fn default_rate_limit_trust_proxy_headers() -> bool {
    false
}

fn default_nats_request_timeout_secs() -> u64 {
    5
}

fn default_nats_storage_timeout_secs() -> u64 {
    30
}

impl ApiConfig {
    pub fn load() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        let config = envy::from_env::<Self>().map_err(anyhow::Error::from)?;

        if config.jwt_secret.len() < 32 {
            anyhow::bail!("JWT_SECRET must be at least 32 characters long");
        }
        if config.master_key.len() < 32 {
            anyhow::bail!("MASTER_KEY must be at least 32 characters long");
        }

        Ok(config)
    }
}
