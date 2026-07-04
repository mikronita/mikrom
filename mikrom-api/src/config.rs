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

    #[serde(default = "default_nats_scheduler_long_timeout_secs")]
    pub nats_scheduler_long_timeout_secs: u64,

    #[serde(default = "default_nats_scheduler_database_timeout_secs")]
    pub nats_scheduler_database_timeout_secs: u64,

    #[serde(default = "default_nats_storage_timeout_secs")]
    pub nats_storage_timeout_secs: u64,

    #[serde(default = "default_acme_email")]
    pub acme_email: String,

    #[serde(default = "default_acme_staging")]
    pub acme_staging: bool,

    #[serde(default = "default_router_tls_hostname")]
    pub router_tls_hostname: String,

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
    pub neon_safekeeper_http_url: Option<String>,
    pub neon_bearer_token: Option<String>,
    pub neon_safekeeper_token: Option<String>,
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
            database_url: "postgres://[::1]/mikrom".to_string(),
            nats_url: "nats://[::1]:4222".to_string(),
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
            nats_scheduler_long_timeout_secs: default_nats_scheduler_long_timeout_secs(),
            nats_scheduler_database_timeout_secs: default_nats_scheduler_database_timeout_secs(),
            nats_storage_timeout_secs: default_nats_storage_timeout_secs(),
            acme_email: default_acme_email(),
            acme_staging: default_acme_staging(),
            router_tls_hostname: default_router_tls_hostname(),
            acme_check_interval: default_acme_check_interval(),
            certs_dir: None,
            github_app_id: None,
            github_client_id: None,
            github_client_secret: None,
            github_private_key: None,
            github_app_slug: None,
            github_webhook_url_base: None,
            neon_pageserver_url: None,
            neon_safekeeper_http_url: None,
            neon_bearer_token: None,
            neon_safekeeper_token: None,
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
    false
}

fn default_router_tls_hostname() -> String {
    "debaser.spluca.org".to_string()
}

fn default_acme_check_interval() -> u64 {
    3600 // 1 hour
}

fn default_api_port() -> u16 {
    5001
}

fn default_router_addr() -> String {
    "http://[::1]:80".to_string()
}

fn default_frontend_url() -> String {
    "https://mikrom.spluca.org".to_string()
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

fn default_nats_scheduler_long_timeout_secs() -> u64 {
    15
}

fn default_nats_scheduler_database_timeout_secs() -> u64 {
    10
}

fn default_nats_storage_timeout_secs() -> u64 {
    30
}

impl ApiConfig {
    pub fn load() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        let config = envy::from_env::<Self>().map_err(anyhow::Error::from)?;

        config.validate()?;

        Ok(config)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.jwt_secret.len() < 32 {
            anyhow::bail!("JWT_SECRET must be at least 32 characters long");
        }
        if self.master_key.len() < 32 {
            anyhow::bail!("MASTER_KEY must be at least 32 characters long");
        }

        if self.router_tls_hostname.trim().is_empty() {
            anyhow::bail!("ROUTER_TLS_HOSTNAME must not be empty");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ApiConfig;

    #[test]
    fn defaults_use_production_acme_and_router_hostname() {
        let config = ApiConfig::default();
        assert!(!config.acme_staging);
        assert_eq!(config.router_tls_hostname, "debaser.spluca.org");
    }

    #[test]
    fn defaults_nats_timeouts_to_expected_values() {
        let config = ApiConfig::default();
        assert_eq!(config.nats_request_timeout_secs, 5);
        assert_eq!(config.nats_scheduler_long_timeout_secs, 15);
        assert_eq!(config.nats_scheduler_database_timeout_secs, 10);
        assert_eq!(config.nats_storage_timeout_secs, 30);
    }

    #[test]
    fn loads_nats_timeouts_from_env() {
        let config: ApiConfig = envy::from_iter(vec![
            (
                "DATABASE_URL".to_string(),
                "postgres://[::1]/mikrom".to_string(),
            ),
            ("NATS_URL".to_string(), "nats://[::1]:4222".to_string()),
            ("JWT_SECRET".to_string(), "x".repeat(32)),
            ("MASTER_KEY".to_string(), "y".repeat(32)),
            (
                "ROUTER_TLS_HOSTNAME".to_string(),
                "router.example.com".to_string(),
            ),
            ("NATS_REQUEST_TIMEOUT_SECS".to_string(), "7".to_string()),
            (
                "NATS_SCHEDULER_LONG_TIMEOUT_SECS".to_string(),
                "19".to_string(),
            ),
            (
                "NATS_SCHEDULER_DATABASE_TIMEOUT_SECS".to_string(),
                "11".to_string(),
            ),
            ("NATS_STORAGE_TIMEOUT_SECS".to_string(), "31".to_string()),
        ])
        .expect("config should deserialize");

        assert_eq!(config.nats_request_timeout_secs, 7);
        assert_eq!(config.nats_scheduler_long_timeout_secs, 19);
        assert_eq!(config.nats_scheduler_database_timeout_secs, 11);
        assert_eq!(config.nats_storage_timeout_secs, 31);
    }

    #[test]
    fn load_rejects_empty_router_tls_hostname() {
        let config = ApiConfig {
            router_tls_hostname: "   ".to_string(),
            ..ApiConfig::default()
        };

        let err = config.validate().unwrap_err();

        assert!(
            err.to_string()
                .contains("ROUTER_TLS_HOSTNAME must not be empty")
        );
    }

    #[test]
    fn validate_rejects_short_jwt_secret() {
        let config = ApiConfig {
            jwt_secret: "short".to_string(),
            ..ApiConfig::default()
        };

        let err = config.validate().unwrap_err();

        assert!(
            err.to_string()
                .contains("JWT_SECRET must be at least 32 characters long")
        );
    }

    #[test]
    fn validate_rejects_short_master_key() {
        let config = ApiConfig {
            master_key: "short".to_string(),
            ..ApiConfig::default()
        };

        let err = config.validate().unwrap_err();

        assert!(
            err.to_string()
                .contains("MASTER_KEY must be at least 32 characters long")
        );
    }
}
