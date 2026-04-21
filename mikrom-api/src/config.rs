use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct ApiConfig {
    pub database_url: String,

    #[serde(default = "default_jwt_secret")]
    pub jwt_secret: String,

    #[serde(default = "default_master_key")]
    pub master_key: String,

    #[serde(default = "default_api_port")]
    pub api_port: u16,

    #[serde(default = "default_scheduler_addr")]
    pub scheduler_addr: String,

    #[serde(default = "default_use_tls")]
    pub use_tls: bool,

    pub certs_dir: Option<String>,
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

fn default_scheduler_addr() -> String {
    "http://127.0.0.1:5002".to_string()
}

fn default_use_tls() -> bool {
    false
}

impl ApiConfig {
    pub fn load() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        envy::from_env::<Self>().map_err(anyhow::Error::from)
    }

    #[must_use]
    pub fn scheduler_config(&self) -> crate::scheduler::SchedulerConfig {
        crate::scheduler::SchedulerConfig {
            addr: self.scheduler_addr.clone(),
            use_tls: self.use_tls,
            certs_dir: self.certs_dir.clone(),
        }
    }
}
