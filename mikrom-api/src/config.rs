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
    pub neon_pageserver_url: Option<String>,
    pub neon_bearer_token: Option<String>,
    #[serde(default)]
    pub neon_jwks_json: Option<String>,
    #[serde(default)]
    pub neon_jwks_path: Option<String>,
    #[serde(default)]
    pub neon_instance_id: Option<String>,
    #[serde(default)]
    pub neon_safekeeper_connstrs: Option<String>,
    #[serde(default)]
    pub mikrom_neon_dev_mode: Option<bool>,
    #[serde(default)]
    pub mikrom_init_trace_files: Option<String>,
    #[serde(default)]
    pub neon_configure_token: Option<String>,
    #[serde(default)]
    pub neon_configure_private_key_pem: Option<String>,
    #[serde(default)]
    pub neon_configure_private_key_path: Option<String>,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            database_url: "postgres://localhost/mikrom".to_string(),
            nats_url: "nats://localhost:4222".to_string(),
            jwt_secret: "default_jwt_secret_at_least_32_chars_long".to_string(),
            master_key: "default_master_key_at_least_32_chars_long".to_string(),
            api_port: default_api_port(),
            router_addr: default_router_addr(),
            frontend_url: default_frontend_url(),
            use_tls: default_use_tls(),
            deployment_env: default_deployment_env(),
            rate_limit_public_rpm: None,
            rate_limit_auth_login_rpm: None,
            rate_limit_auth_register_rpm: None,
            rate_limit_github_install_rpm: None,
            rate_limit_apps_create_rpm: None,
            rate_limit_apps_deploy_rpm: None,
            rate_limit_webhooks_github_generic_rpm: None,
            rate_limit_webhooks_github_named_rpm: None,
            rate_limit_authenticated_read_rpm: None,
            rate_limit_authenticated_write_rpm: None,
            rate_limit_authenticated_stream_rpm: None,
            rate_limit_entry_ttl_secs: default_rate_limit_entry_ttl_secs(),
            rate_limit_cleanup_interval_secs: default_rate_limit_cleanup_interval_secs(),
            rate_limit_trust_proxy_headers: default_rate_limit_trust_proxy_headers(),
            nats_request_timeout_secs: default_nats_request_timeout_secs(),
            nats_storage_timeout_secs: default_nats_storage_timeout_secs(),
            acme_email: default_acme_email(),
            acme_staging: default_acme_staging(),
            acme_check_interval: default_acme_check_interval(),
            certs_dir: None,
            github_app_id: None,
            github_client_id: None,
            github_client_secret: None,
            github_private_key: None,
            github_app_slug: None,
            github_webhook_url_base: None,
            neon_pageserver_url: None,
            neon_bearer_token: None,
            neon_jwks_json: None,
            neon_jwks_path: None,
            neon_instance_id: None,
            neon_safekeeper_connstrs: None,
            mikrom_neon_dev_mode: None,
            mikrom_init_trace_files: None,
            neon_configure_token: None,
            neon_configure_private_key_pem: None,
            neon_configure_private_key_path: None,
        }
    }
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
