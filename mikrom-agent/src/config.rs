use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize, Clone)]
pub struct AgentConfig {
    pub nats_url: String,

    #[serde(default = "default_host_id")]
    pub host_id: String,

    #[serde(default = "default_use_tls")]
    pub use_tls: bool,

    #[serde(default = "default_agent_port")]
    pub agent_port: u16,

    #[serde(default = "default_bridge_ip")]
    pub bridge_ip: String,

    #[serde(default = "default_certs_dir")]
    pub certs_dir: String,

    pub agent_hostname: Option<String>,
}

fn default_certs_dir() -> String {
    "/certs/agent".to_string()
}

fn default_bridge_ip() -> String {
    "10.0.0.1/8".to_string()
}

fn default_host_id() -> String {
    Uuid::new_v4().to_string()
}

fn default_use_tls() -> bool {
    false
}

fn default_agent_port() -> u16 {
    5003
}

impl AgentConfig {
    pub fn load() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        envy::from_env::<Self>().map_err(anyhow::Error::from)
    }

    #[must_use]
    pub fn hostname(&self) -> String {
        self.agent_hostname
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                hostname::get().map_or_else(
                    |_| "unknown".to_string(),
                    |h| h.to_string_lossy().to_string(),
                )
            })
    }
}
