use serde::Deserialize;
use std::path::Path;
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

    #[serde(default = "default_cloud_hypervisor_enabled")]
    pub cloud_hypervisor_enabled: bool,

    #[serde(default = "default_cloud_hypervisor_binary")]
    pub cloud_hypervisor_binary: PathBuf,

    #[serde(default = "default_cloud_hypervisor_kernel")]
    pub cloud_hypervisor_kernel: PathBuf,

    #[serde(default = "default_cloud_hypervisor_base_rootfs")]
    pub cloud_hypervisor_base_rootfs: PathBuf,

    #[serde(default = "default_http_port")]
    pub http_port: u16,

    #[serde(default = "default_max_vms_per_host")]
    pub max_vms_per_host: u32,

    #[serde(default = "default_nats_flapping_session_secs")]
    pub nats_flapping_session_secs: u64,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            nats_url: "nats://localhost:4222".to_string(),
            host_id: "test-host".to_string(),
            use_tls: false,
            bridge_ip: "10.0.0.1/8".to_string(),
            certs_dir: "/certs/agent".to_string(),
            data_path: PathBuf::from("/tmp/mikrom-test"),
            agent_hostname: None,
            agent_advertise_address: None,
            wireguard_port: Some(51820),
            wireguard_pubkey: None,
            cloud_hypervisor_enabled: true,
            cloud_hypervisor_binary: PathBuf::from("/usr/bin/cloud-hypervisor"),
            cloud_hypervisor_kernel: PathBuf::from("/opt/cloud-hypervisor/vmlinux.bin"),
            cloud_hypervisor_base_rootfs: PathBuf::from("/opt/cloud-hypervisor/base-rootfs.ext4"),
            http_port: 5002,
            max_vms_per_host: 0,
            nats_flapping_session_secs: 30,
        }
    }
}

const fn default_cloud_hypervisor_enabled() -> bool {
    true
}

fn default_cloud_hypervisor_binary() -> PathBuf {
    PathBuf::from("/usr/bin/cloud-hypervisor")
}

fn default_cloud_hypervisor_kernel() -> PathBuf {
    PathBuf::from("/opt/cloud-hypervisor/vmlinux.bin")
}

fn default_cloud_hypervisor_base_rootfs() -> PathBuf {
    PathBuf::from("/opt/cloud-hypervisor/base-rootfs.ext4")
}

const fn default_http_port() -> u16 {
    5002
}

const fn default_max_vms_per_host() -> u32 {
    0 // 0 = unlimited
}

const fn default_nats_flapping_session_secs() -> u64 {
    30
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
        let env_host_id = std::env::var("AGENT_HOST_ID")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let mut config: AgentConfig = envy::from_env()?;

        // Ensure data path exists
        if !config.data_path.exists() {
            let _ = std::fs::create_dir_all(&config.data_path);
        }

        config.host_id = match env_host_id {
            Some(host_id) => {
                persist_host_id(&config.data_path, &host_id)?;
                host_id
            },
            None => load_or_persist_host_id(&config.data_path, config.host_id.clone())?,
        };

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

        config.validate()?;

        Ok(config)
    }

    /// Validate that the loaded configuration is usable.
    ///
    /// Checks:
    /// - NATS URL is parseable
    /// - Data path exists and is writable
    /// - Host ID is non-empty
    /// - Required capabilities are present (warns only)
    pub fn validate(&self) -> anyhow::Result<()> {
        // 1. NATS URL must be parseable
        if self.nats_url.is_empty() {
            return Err(anyhow::anyhow!("NATS URL is empty"));
        }
        if let Err(e) = self.nats_url.parse::<std::net::SocketAddr>() {
            // Not a bare SocketAddr — try as URL (async_nats accepts both)
            if !self.nats_url.starts_with("nats://") && !self.nats_url.starts_with("tls://") {
                return Err(anyhow::anyhow!(
                    "NATS URL '{}' does not look like a valid NATS endpoint: {e}",
                    self.nats_url
                ));
            }
        }

        // 2. Data path must exist and be writable
        if !self.data_path.exists() {
            return Err(anyhow::anyhow!(
                "Data path '{}' does not exist",
                self.data_path.display()
            ));
        }
        let test_file = self.data_path.join(".write_test");
        match std::fs::File::create(&test_file) {
            Ok(_) => {
                let _ = std::fs::remove_file(&test_file);
            },
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "Data path '{}' is not writable: {e}",
                    self.data_path.display()
                ));
            },
        }

        // 3. Host ID must be non-empty
        if self.host_id.is_empty() {
            return Err(anyhow::anyhow!("Host ID is empty"));
        }

        // 4. Warn if we don't seem to have CAP_NET_ADMIN (needed for bridges/TAPs)
        #[cfg(target_os = "linux")]
        {
            let cap_file = std::path::Path::new("/proc/self/status");
            if cap_file.exists()
                && let Ok(content) = std::fs::read_to_string(cap_file)
                && let Some(line) = content.lines().find(|l| l.starts_with("CapEff:"))
            {
                let caps_hex = line.trim_start_matches("CapEff:").trim();
                if let Ok(caps) = u64::from_str_radix(caps_hex, 16) {
                    const CAP_NET_ADMIN: u64 = 1 << 12;
                    if caps & CAP_NET_ADMIN == 0 {
                        tracing::warn!(
                            "Process lacks CAP_NET_ADMIN — bridge and TAP creation may fail"
                        );
                    }
                }
            }
        }

        tracing::info!("Agent configuration validated successfully");
        Ok(())
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

fn host_id_path(data_path: &Path) -> PathBuf {
    data_path.join("host_id.txt")
}

fn load_or_persist_host_id(data_path: &Path, generated_host_id: String) -> anyhow::Result<String> {
    let path = host_id_path(data_path);
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let existing = existing.trim().to_string();
        if !existing.is_empty() {
            return Ok(existing);
        }
    }

    persist_host_id(data_path, &generated_host_id)?;
    Ok(generated_host_id)
}

fn persist_host_id(data_path: &Path, host_id: &str) -> anyhow::Result<()> {
    std::fs::write(host_id_path(data_path), host_id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_id_is_persisted_between_loads() {
        let data_path =
            std::env::temp_dir().join(format!("mikrom-agent-config-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&data_path).unwrap();

        let first = load_or_persist_host_id(&data_path, "host-a".to_string()).unwrap();
        let second = load_or_persist_host_id(&data_path, "host-b".to_string()).unwrap();

        assert_eq!(first, "host-a");
        assert_eq!(second, "host-a");

        let _ = std::fs::remove_dir_all(&data_path);
    }

    #[test]
    fn default_nats_flapping_session_secs_is_reasonable() {
        let config = AgentConfig {
            nats_url: "nats://localhost:4222".to_string(),
            host_id: "host-1".to_string(),
            use_tls: false,
            bridge_ip: "10.0.0.1/8".to_string(),
            certs_dir: "/certs/agent".to_string(),
            data_path: std::env::temp_dir(),
            agent_hostname: None,
            agent_advertise_address: None,
            wireguard_port: None,
            wireguard_pubkey: None,
            cloud_hypervisor_enabled: true,
            cloud_hypervisor_binary: PathBuf::from("/usr/bin/cloud-hypervisor"),
            cloud_hypervisor_kernel: PathBuf::from("/opt/cloud-hypervisor/vmlinux.bin"),
            cloud_hypervisor_base_rootfs: PathBuf::from("/opt/cloud-hypervisor/base-rootfs.ext4"),
            http_port: 5002,
            max_vms_per_host: 0,
            nats_flapping_session_secs: default_nats_flapping_session_secs(),
        };

        assert_eq!(config.nats_flapping_session_secs, 30);
    }
}
