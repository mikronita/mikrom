use serde::Deserialize;
use std::path::PathBuf;
use uuid::Uuid;
use x25519_dalek::{PublicKey, StaticSecret};

#[derive(Debug, Deserialize, Clone)]
pub struct AgentConfig {
    pub nats_url: String,

    #[serde(default = "default_host_id")]
    pub host_id: String,

    #[serde(default = "default_use_tls")]
    pub use_tls: bool,

    #[serde(default = "default_bridge_ip")]
    pub bridge_ip: String,

    #[serde(default = "default_certs_dir")]
    pub certs_dir: String,

    #[serde(default = "default_data_path")]
    pub data_path: PathBuf,

    pub agent_hostname: Option<String>,

    pub agent_advertise_address: Option<String>,

    pub wireguard_port: Option<u16>,

    pub wireguard_pubkey: Option<String>,
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

fn default_data_path() -> PathBuf {
    PathBuf::from("/var/lib/mikrom-agent")
}

impl AgentConfig {
    pub fn load() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        let mut config: AgentConfig = envy::from_env()?;

        // Ensure data path exists
        if !config.data_path.exists() {
            let _ = std::fs::create_dir_all(&config.data_path);
        }

        // Ensure WireGuard key pair exists
        let key_path = config.data_path.join("wireguard.key");
        let secret = if let Ok(key_str) = std::fs::read_to_string(&key_path) {
            let key_str = key_str.trim();
            let key_bytes = if key_str.len() == 64 {
                // Legacy hex format
                hex::decode(key_str)?
            } else {
                // Standard base64 format
                use base64::{Engine as _, engine::general_purpose};
                general_purpose::STANDARD.decode(key_str)?
            };

            let mut array = [0u8; 32];
            if key_bytes.len() == 32 {
                array.copy_from_slice(&key_bytes);
            } else {
                return Err(anyhow::anyhow!("Invalid WireGuard key length"));
            }
            StaticSecret::from(array)
        } else {
            let s = StaticSecret::random_from_rng(rand::thread_rng());
            use base64::{Engine as _, engine::general_purpose};
            let key_base64 = general_purpose::STANDARD.encode(s.to_bytes());
            if std::fs::write(&key_path, &key_base64).is_ok() {
                tracing::info!("Generated new WireGuard key pair");
            }
            s
        };

        let public = PublicKey::from(&secret);
        use base64::{Engine as _, engine::general_purpose};
        config.wireguard_pubkey = Some(general_purpose::STANDARD.encode(public.as_bytes()));

        Ok(config)
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

    pub fn get_wg_private_key(&self) -> Option<String> {
        let key_path = self.data_path.join("wireguard.key");
        let key_str = std::fs::read_to_string(key_path).ok()?.trim().to_string();
        if key_str.len() == 64 {
            // Legacy hex format, convert to base64 for wg command
            let bytes = hex::decode(&key_str).ok()?;
            use base64::{Engine as _, engine::general_purpose};
            Some(general_purpose::STANDARD.encode(bytes))
        } else {
            Some(key_str)
        }
    }
}
