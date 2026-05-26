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

    #[serde(default = "default_rps_limit")]
    pub rps_limit: isize,

    #[serde(default = "default_router_threads")]
    pub router_threads: usize,
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

const fn default_rps_limit() -> isize {
    100
}

fn default_router_threads() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
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
            rps_limit: 100,
            router_threads: 1,
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
            rps_limit: 100,
            router_threads: 1,
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
            rps_limit: 100,
            router_threads: 1,
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
            rps_limit: 100,
            router_threads: 1,
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
            rps_limit: 0,
            router_threads: 1,
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
            rps_limit: 100,
            router_threads: 1,
        };

        assert!(config.validate().is_err());
    }
}
