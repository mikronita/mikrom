use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct SchedulerConfig {
    pub database_url: String,
    pub nats_url: String,

    #[serde(default = "default_database_max_connections")]
    pub database_max_connections: u32,

    #[serde(default = "default_use_tls")]
    pub use_tls: bool,

    #[serde(default = "default_certs_dir")]
    pub certs_dir: String,
}

fn default_use_tls() -> bool {
    false
}

fn default_database_max_connections() -> u32 {
    10
}

fn default_certs_dir() -> String {
    "/certs/scheduler".to_string()
}

impl SchedulerConfig {
    pub fn load() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        envy::from_env::<Self>().map_err(anyhow::Error::from)
    }
}
