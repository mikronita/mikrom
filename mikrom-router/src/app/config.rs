use serde::Deserialize;
use std::path::PathBuf;

macro_rules! string_newtype {
    ($name:ident) => {
        #[derive(Debug, Clone, Deserialize, PartialEq, Eq, Hash)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            #[must_use]
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_string())
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

string_newtype!(DatabaseUrl);
string_newtype!(NatsUrl);
string_newtype!(MasterKey);
string_newtype!(RouterId);

#[derive(Debug, Clone, Deserialize)]
pub struct RouterConfig {
    pub database_url: DatabaseUrl,
    pub nats_url: NatsUrl,

    #[serde(default = "default_nats_use_tls")]
    pub nats_use_tls: bool,

    pub nats_certs_dir: Option<String>,
    pub upstream_ca_certs_dir: Option<String>,
    pub master_key: MasterKey,

    #[serde(default = "default_router_id")]
    pub router_id: RouterId,

    pub advertise_address: Option<String>,

    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,

    pub state_cache_path: Option<PathBuf>,

    #[serde(default = "default_wireguard_port")]
    pub wireguard_port: u16,

    #[serde(default = "default_acme_staging")]
    pub acme_staging: bool,

    #[serde(default = "default_api_host")]
    pub api_host: String,

    #[serde(default = "default_api_upstream_url")]
    pub api_upstream_url: String,

    #[serde(default = "default_dashboard_host")]
    pub dashboard_host: String,

    #[serde(default = "default_dashboard_upstream_url")]
    pub dashboard_upstream_url: String,

    #[serde(default = "default_default_site_host")]
    pub default_site_host: String,

    #[serde(default = "default_default_site_redirect_url")]
    pub default_site_redirect_url: String,

    #[serde(default = "default_rps_limit")]
    pub rps_limit: isize,

    #[serde(default = "default_router_threads")]
    pub router_threads: usize,

    #[serde(default = "default_startup_connect_timeout_secs")]
    pub startup_connect_timeout_secs: u64,

    #[serde(default = "default_downstream_request_timeout_secs")]
    pub downstream_request_timeout_secs: u64,

    #[serde(default = "default_downstream_response_timeout_secs")]
    pub downstream_response_timeout_secs: u64,

    #[serde(default = "default_upstream_connect_timeout_secs")]
    pub upstream_connect_timeout_secs: u64,

    #[serde(default = "default_upstream_read_timeout_secs")]
    pub upstream_read_timeout_secs: u64,

    #[serde(default = "default_upstream_write_timeout_secs")]
    pub upstream_write_timeout_secs: u64,

    #[serde(default = "default_upstream_idle_timeout_secs")]
    pub upstream_idle_timeout_secs: u64,

    #[serde(default = "default_route_wait_timeout_secs")]
    pub route_wait_timeout_secs: u64,
}

const fn default_nats_use_tls() -> bool {
    false
}

fn default_router_id() -> RouterId {
    hostname::get().map_or_else(
        |_| RouterId::from("unknown-router"),
        |hostname| RouterId::from(hostname.to_string_lossy().into_owned()),
    )
}

fn default_data_dir() -> PathBuf {
    PathBuf::from("/var/lib/mikrom")
}

const fn default_wireguard_port() -> u16 {
    51822
}

const fn default_acme_staging() -> bool {
    false
}

fn default_api_host() -> String {
    "api.mikrom.spluca.org".to_string()
}

fn default_api_upstream_url() -> String {
    "http://[::1]:5001".to_string()
}

fn default_dashboard_host() -> String {
    "dashboard.mikrom.spluca.org".to_string()
}

fn default_dashboard_upstream_url() -> String {
    "http://[::1]:3000".to_string()
}

fn default_default_site_host() -> String {
    "debaser.spluca.org".to_string()
}

fn default_default_site_redirect_url() -> String {
    "https://spluca.org/".to_string()
}

const fn default_rps_limit() -> isize {
    100
}

fn default_router_threads() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}

const fn default_startup_connect_timeout_secs() -> u64 {
    5
}

const fn default_downstream_request_timeout_secs() -> u64 {
    10
}

const fn default_downstream_response_timeout_secs() -> u64 {
    30
}

const fn default_upstream_connect_timeout_secs() -> u64 {
    5
}

const fn default_upstream_read_timeout_secs() -> u64 {
    30
}

const fn default_upstream_write_timeout_secs() -> u64 {
    30
}

const fn default_upstream_idle_timeout_secs() -> u64 {
    60
}

const fn default_route_wait_timeout_secs() -> u64 {
    30
}

fn timeout_duration(secs: u64) -> std::time::Duration {
    std::time::Duration::from_secs(secs.max(1))
}

impl RouterConfig {
    #[must_use]
    pub fn startup_connect_timeout(&self) -> std::time::Duration {
        timeout_duration(self.startup_connect_timeout_secs)
    }

    #[must_use]
    pub fn downstream_request_timeout(&self) -> std::time::Duration {
        timeout_duration(self.downstream_request_timeout_secs)
    }

    #[must_use]
    pub fn downstream_response_timeout(&self) -> std::time::Duration {
        timeout_duration(self.downstream_response_timeout_secs)
    }

    #[must_use]
    pub fn upstream_connect_timeout(&self) -> std::time::Duration {
        timeout_duration(self.upstream_connect_timeout_secs)
    }

    #[must_use]
    pub fn upstream_read_timeout(&self) -> std::time::Duration {
        timeout_duration(self.upstream_read_timeout_secs)
    }

    #[must_use]
    pub fn upstream_write_timeout(&self) -> std::time::Duration {
        timeout_duration(self.upstream_write_timeout_secs)
    }

    #[must_use]
    pub fn upstream_idle_timeout(&self) -> std::time::Duration {
        timeout_duration(self.upstream_idle_timeout_secs)
    }

    #[must_use]
    pub fn route_wait_timeout(&self) -> std::time::Duration {
        timeout_duration(self.route_wait_timeout_secs)
    }
}

impl RouterConfig {
    pub fn load() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();

        let mut config: Self = envy::from_env()?;

        if config.nats_certs_dir.is_none() {
            config.nats_certs_dir = std::env::var("CERTS_DIR").ok();
        }

        if config.state_cache_path.is_none() {
            config.state_cache_path = Some(config.data_dir.join("router-state.json"));
        }

        if config.advertise_address.is_none() {
            config.advertise_address = Some(config.router_id.as_str().to_string());
        }

        config.validate()?;

        if !config.data_dir.exists() {
            std::fs::create_dir_all(&config.data_dir).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to create router data directory {}: {e}",
                    config.data_dir.display()
                )
            })?;
        }

        if let Some(path) = &config.state_cache_path
            && let Some(parent) = path.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to create parent directory for state cache {}: {e}",
                    path.display()
                )
            })?;
        }

        Ok(config)
    }

    #[must_use]
    pub const fn state_cache_path(&self) -> &PathBuf {
        self.state_cache_path
            .as_ref()
            .expect("state cache path is always initialized in load()")
    }

    #[must_use]
    pub fn advertise_address(&self) -> &str {
        self.advertise_address
            .as_deref()
            .unwrap_or(self.router_id.as_str())
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.database_url.as_str().trim().is_empty() {
            return Err(anyhow::anyhow!("DATABASE_URL cannot be empty"));
        }

        if self.nats_url.as_str().trim().is_empty() {
            return Err(anyhow::anyhow!("NATS_URL cannot be empty"));
        }

        if self.router_id.as_str().trim().is_empty() {
            return Err(anyhow::anyhow!("ROUTER_ID cannot be empty"));
        }

        if self.master_key.as_str().trim().is_empty() {
            return Err(anyhow::anyhow!("MASTER_KEY cannot be empty"));
        }

        if self
            .advertise_address
            .as_deref()
            .is_some_and(|address| address.trim().is_empty())
        {
            return Err(anyhow::anyhow!("ADVERTISE_ADDRESS cannot be empty"));
        }

        if self.router_threads == 0 {
            return Err(anyhow::anyhow!("ROUTER_THREADS must be greater than zero"));
        }

        if self.rps_limit <= 0 {
            return Err(anyhow::anyhow!("RPS_LIMIT must be greater than zero"));
        }

        if self.api_host.trim().is_empty() {
            return Err(anyhow::anyhow!("API_HOST cannot be empty"));
        }

        if self.api_upstream_url.trim().is_empty() {
            return Err(anyhow::anyhow!("API_UPSTREAM_URL cannot be empty"));
        }

        if self.dashboard_host.trim().is_empty() {
            return Err(anyhow::anyhow!("DASHBOARD_HOST cannot be empty"));
        }

        if self.dashboard_upstream_url.trim().is_empty() {
            return Err(anyhow::anyhow!("DASHBOARD_UPSTREAM_URL cannot be empty"));
        }

        if self.default_site_host.trim().is_empty() {
            return Err(anyhow::anyhow!("DEFAULT_SITE_HOST cannot be empty"));
        }

        if self.default_site_redirect_url.trim().is_empty() {
            return Err(anyhow::anyhow!("DEFAULT_SITE_REDIRECT_URL cannot be empty"));
        }

        if self.wireguard_port == 0 {
            return Err(anyhow::anyhow!("WIREGUARD_PORT must be greater than zero"));
        }

        if self.nats_use_tls && self.nats_certs_dir.is_none() {
            return Err(anyhow::anyhow!(
                "NATS_USE_TLS is enabled but no NATS_CERTS_DIR or CERTS_DIR was configured"
            ));
        }

        if self
            .nats_certs_dir
            .as_deref()
            .is_some_and(|path| path.trim().is_empty())
        {
            return Err(anyhow::anyhow!(
                "NATS_CERTS_DIR/CERTS_DIR cannot be empty when configured"
            ));
        }

        if self
            .upstream_ca_certs_dir
            .as_deref()
            .is_some_and(|path| path.trim().is_empty())
        {
            return Err(anyhow::anyhow!(
                "UPSTREAM_CA_CERTS_DIR cannot be empty when configured"
            ));
        }

        if let Some(path) = &self.state_cache_path
            && let Some(parent) = path.parent()
            && parent.as_os_str().is_empty()
        {
            return Err(anyhow::anyhow!(
                "STATE_CACHE_PATH must have a valid parent directory"
            ));
        }

        Ok(())
    }
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            database_url: DatabaseUrl::from("postgres://localhost/router"),
            nats_url: NatsUrl::from("nats://localhost:4222"),
            nats_use_tls: default_nats_use_tls(),
            nats_certs_dir: None,
            upstream_ca_certs_dir: None,
            master_key: MasterKey::from("router-test-key"),
            router_id: default_router_id(),
            advertise_address: None,
            data_dir: default_data_dir(),
            state_cache_path: None,
            wireguard_port: default_wireguard_port(),
            acme_staging: default_acme_staging(),
            api_host: default_api_host(),
            api_upstream_url: default_api_upstream_url(),
            dashboard_host: default_dashboard_host(),
            dashboard_upstream_url: default_dashboard_upstream_url(),
            default_site_host: default_default_site_host(),
            default_site_redirect_url: default_default_site_redirect_url(),
            rps_limit: default_rps_limit(),
            router_threads: default_router_threads(),
            startup_connect_timeout_secs: default_startup_connect_timeout_secs(),
            downstream_request_timeout_secs: default_downstream_request_timeout_secs(),
            downstream_response_timeout_secs: default_downstream_response_timeout_secs(),
            upstream_connect_timeout_secs: default_upstream_connect_timeout_secs(),
            upstream_read_timeout_secs: default_upstream_read_timeout_secs(),
            upstream_write_timeout_secs: default_upstream_write_timeout_secs(),
            upstream_idle_timeout_secs: default_upstream_idle_timeout_secs(),
            route_wait_timeout_secs: default_route_wait_timeout_secs(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DatabaseUrl, MasterKey, NatsUrl, RouterConfig, RouterId};
    use std::path::PathBuf;

    #[test]
    fn validate_rejects_empty_urls() {
        let config = RouterConfig {
            database_url: DatabaseUrl::from(""),
            nats_url: NatsUrl::from(""),
            nats_use_tls: false,
            nats_certs_dir: None,
            upstream_ca_certs_dir: None,
            master_key: MasterKey::from(""),
            router_id: RouterId::from("router-1"),
            advertise_address: None,
            data_dir: PathBuf::from("/tmp"),
            state_cache_path: None,
            wireguard_port: 51822,
            acme_staging: false,
            default_site_host: "debaser.spluca.org".to_string(),
            default_site_redirect_url: "https://spluca.org/".to_string(),
            rps_limit: 100,
            router_threads: 1,
            ..Default::default()
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn validate_rejects_empty_master_key() {
        let config = RouterConfig {
            database_url: DatabaseUrl::from("postgres://localhost/router"),
            nats_url: NatsUrl::from("nats://localhost:4222"),
            nats_use_tls: false,
            nats_certs_dir: None,
            upstream_ca_certs_dir: None,
            master_key: MasterKey::from(""),
            router_id: RouterId::from("router-1"),
            advertise_address: None,
            data_dir: PathBuf::from("/tmp"),
            state_cache_path: None,
            wireguard_port: 51822,
            acme_staging: false,
            default_site_host: "debaser.spluca.org".to_string(),
            default_site_redirect_url: "https://spluca.org/".to_string(),
            rps_limit: 100,
            router_threads: 1,
            ..Default::default()
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn advertise_address_defaults_to_router_id() {
        let config = RouterConfig {
            database_url: DatabaseUrl::from("postgres://localhost/router"),
            nats_url: NatsUrl::from("nats://localhost:4222"),
            nats_use_tls: false,
            nats_certs_dir: None,
            upstream_ca_certs_dir: None,
            master_key: MasterKey::from("key"),
            router_id: RouterId::from("router-1"),
            advertise_address: None,
            data_dir: PathBuf::from("/tmp"),
            state_cache_path: None,
            wireguard_port: 51822,
            acme_staging: false,
            default_site_host: "debaser.spluca.org".to_string(),
            default_site_redirect_url: "https://spluca.org/".to_string(),
            rps_limit: 100,
            router_threads: 1,
            ..Default::default()
        };

        assert_eq!(config.advertise_address(), "router-1");
    }

    #[test]
    fn validate_rejects_blank_advertise_address() {
        let config = RouterConfig {
            database_url: DatabaseUrl::from("postgres://localhost/router"),
            nats_url: NatsUrl::from("nats://localhost:4222"),
            nats_use_tls: false,
            nats_certs_dir: None,
            upstream_ca_certs_dir: None,
            master_key: MasterKey::from("key"),
            router_id: RouterId::from("router-1"),
            advertise_address: Some("   ".to_string()),
            data_dir: PathBuf::from("/tmp"),
            state_cache_path: None,
            wireguard_port: 51822,
            acme_staging: false,
            default_site_host: "debaser.spluca.org".to_string(),
            default_site_redirect_url: "https://spluca.org/".to_string(),
            rps_limit: 100,
            router_threads: 1,
            ..Default::default()
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn validate_rejects_nonpositive_rps_limit() {
        let config = RouterConfig {
            database_url: DatabaseUrl::from("postgres://localhost/router"),
            nats_url: NatsUrl::from("nats://localhost:4222"),
            nats_use_tls: false,
            nats_certs_dir: None,
            upstream_ca_certs_dir: None,
            master_key: MasterKey::from("key"),
            router_id: RouterId::from("router-1"),
            advertise_address: None,
            data_dir: PathBuf::from("/tmp"),
            state_cache_path: None,
            wireguard_port: 51822,
            acme_staging: false,
            default_site_host: "debaser.spluca.org".to_string(),
            default_site_redirect_url: "https://spluca.org/".to_string(),
            rps_limit: 0,
            router_threads: 1,
            ..Default::default()
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn validate_rejects_blank_certs_directories() {
        let config = RouterConfig {
            database_url: DatabaseUrl::from("postgres://localhost/router"),
            nats_url: NatsUrl::from("nats://localhost:4222"),
            nats_use_tls: false,
            nats_certs_dir: Some(" ".to_string()),
            upstream_ca_certs_dir: Some(String::new()),
            master_key: MasterKey::from("key"),
            router_id: RouterId::from("router-1"),
            advertise_address: None,
            data_dir: PathBuf::from("/tmp"),
            state_cache_path: None,
            wireguard_port: 51822,
            acme_staging: false,
            default_site_host: "debaser.spluca.org".to_string(),
            default_site_redirect_url: "https://spluca.org/".to_string(),
            rps_limit: 100,
            router_threads: 1,
            ..Default::default()
        };

        assert!(config.validate().is_err());
    }
}
