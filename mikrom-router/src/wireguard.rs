#![allow(
    clippy::must_use_candidate,
    clippy::return_self_not_must_use,
    clippy::missing_const_for_fn,
    clippy::items_after_statements,
    clippy::uninlined_format_args,
    clippy::cast_lossless,
    clippy::option_if_let_else,
    clippy::format_push_string,
    clippy::collapsible_if,
    clippy::redundant_closure_for_method_calls
)]

use tokio::process::Command;
use tracing::{debug, info, warn};

pub struct WireGuardManager {
    interface: String,
    config_dir: String,
    listen_port: u16,
}

impl WireGuardManager {
    pub fn new(interface: &str) -> Self {
        Self {
            interface: interface.to_string(),
            config_dir: "/etc/wireguard".to_string(),
            listen_port: 51821,
        }
    }

    pub fn with_listen_port(mut self, port: u16) -> Self {
        self.listen_port = port;
        self
    }

    pub fn with_config_dir(mut self, dir: &str) -> Self {
        self.config_dir = dir.to_string();
        self
    }

    pub async fn load_or_generate_key(&self, data_dir: &str) -> anyhow::Result<String> {
        let key_path = std::path::Path::new(data_dir).join("wg.key");

        if key_path.exists() {
            let key = tokio::fs::read_to_string(&key_path).await?;
            return Ok(key.trim().to_string());
        }

        info!("Generating new WireGuard private key...");
        let secret = x25519_dalek::StaticSecret::random_from_rng(rand::thread_rng());
        let priv_bytes = secret.to_bytes();

        // Convert to base64 for WireGuard
        use base64::{Engine as _, engine::general_purpose};
        let priv_b64 = general_purpose::STANDARD.encode(priv_bytes);

        tokio::fs::write(&key_path, &priv_b64).await?;
        // Set permissions to 600
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).await?;

        Ok(priv_b64)
    }

    pub fn get_public_key(&self, private_key: &str) -> anyhow::Result<String> {
        use base64::{Engine as _, engine::general_purpose};
        let priv_bytes = general_purpose::STANDARD.decode(private_key.trim())?;
        let secret =
            x25519_dalek::StaticSecret::from(<[u8; 32]>::try_from(priv_bytes).map_err(|_| {
                anyhow::anyhow!("Invalid private key length (expected 32 bytes decoded)")
            })?);
        let public = x25519_dalek::PublicKey::from(&secret);
        Ok(general_purpose::STANDARD.encode(public.as_bytes()))
    }

    pub async fn init(&self, private_key: &str, host_id: &str) -> anyhow::Result<()> {
        let host_ipv6 = self.get_host_ipv6(host_id);

        // 1. Create baseline config for wg setconf
        let conf = format!(
            "[Interface]\nPrivateKey = {}\nListenPort = {}\n",
            private_key, self.listen_port
        );

        let conf_path = format!("{}/{}.conf", self.config_dir, self.interface);
        let _ = tokio::fs::create_dir_all(&self.config_dir).await;

        tokio::fs::write(&conf_path, &conf).await?;
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&conf_path, std::fs::Permissions::from_mode(0o600)).await?;

        // 2. Initialize interface with ip and wg
        info!("Initializing WireGuard interface {}", self.interface);

        // Remove if exists
        let _ = Command::new("ip")
            .args(["link", "del", "dev", &self.interface])
            .status()
            .await;

        // Add interface
        let status = Command::new("ip")
            .args(["link", "add", "dev", &self.interface, "type", "wireguard"])
            .status()
            .await?;
        if !status.success() {
            return Err(anyhow::anyhow!(
                "Failed to add wireguard interface {}",
                self.interface
            ));
        }

        // Set configuration
        let status = Command::new("wg")
            .args(["setconf", &self.interface, &conf_path])
            .status()
            .await?;
        if !status.success() {
            return Err(anyhow::anyhow!("Failed to setconf for {}", self.interface));
        }

        // Add IP address
        let status = Command::new("ip")
            .args([
                "addr",
                "add",
                &format!("{}/64", host_ipv6),
                "dev",
                &self.interface,
            ])
            .status()
            .await?;
        if !status.success() {
            // Might fail if already assigned, but since we deleted the link it should be fine.
        }

        // Set up
        let status = Command::new("ip")
            .args(["link", "set", "up", "dev", &self.interface])
            .status()
            .await?;
        if !status.success() {
            return Err(anyhow::anyhow!("Failed to bring up {}", self.interface));
        }

        info!(
            "WireGuard interface {} initialized with IP {}",
            self.interface, host_ipv6
        );
        Ok(())
    }

    pub fn get_host_ipv6(&self, host_id: &str) -> String {
        // Use a stable hash function instead of DefaultHasher
        // DJB2 hash algorithm (simple, stable, fast)
        let mut hash: u64 = 5381;
        for c in host_id.bytes() {
            hash = ((hash << 5).wrapping_add(hash)) ^ (c as u64);
        }

        // Use 32 bits of the hash to create a 'normal' looking IPv6 (fd00::xxxx:xxxx)
        let s1 = (hash >> 16) & 0xFFFF;
        let s2 = hash & 0xFFFF;

        format!("fd00::{:x}:{:x}", s1, s2)
    }

    pub async fn update_peers(
        &self,
        peers: &[mikrom_proto::scheduler::Peer],
        private_key: &str,
        host_id: &str,
    ) -> anyhow::Result<()> {
        let mut conf = format!(
            "[Interface]\nPrivateKey = {}\nListenPort = {}\n\n",
            private_key, self.listen_port
        );

        let mut route_targets = Vec::new();

        for peer in peers {
            if peer.wireguard_pubkey.is_empty() || peer.endpoint.is_empty() {
                continue;
            }

            // Normalize public key: if it's hex, convert to base64
            let pubkey = if peer.wireguard_pubkey.len() == 64
                && peer.wireguard_pubkey.chars().all(|c| c.is_ascii_hexdigit())
            {
                if let Ok(bytes) = hex::decode(&peer.wireguard_pubkey) {
                    use base64::{Engine as _, engine::general_purpose};
                    general_purpose::STANDARD.encode(bytes)
                } else {
                    peer.wireguard_pubkey.clone()
                }
            } else {
                peer.wireguard_pubkey.clone()
            };

            // AllowedIPs: Ensure every IP has a prefix length, but don't double-prefix
            let formatted_allowed_ips = Self::normalize_allowed_ips(&peer.allowed_ips);

            let allowed_ips = if formatted_allowed_ips.is_empty() {
                "fd00::/8".to_string()
            } else {
                formatted_allowed_ips.join(",")
            };

            route_targets.extend(formatted_allowed_ips.iter().cloned());

            conf.push_str("[Peer]\n");
            conf.push_str(&format!("PublicKey = {}\n", pubkey));
            conf.push_str(&format!(
                "Endpoint = {}:{}\n",
                peer.endpoint, peer.wireguard_port
            ));
            conf.push_str(&format!("AllowedIPs = {}\n", allowed_ips));
            conf.push_str("PersistentKeepalive = 25\n\n");
        }

        let conf_path = format!("{}/{}.conf", self.config_dir, self.interface);

        // Idempotency check: if the config hasn't changed, do nothing
        if let Ok(existing_conf) = tokio::fs::read_to_string(&conf_path).await {
            if existing_conf == conf {
                debug!(
                    "WireGuard config for {} is unchanged, skipping sync",
                    self.interface
                );
                self.sync_routes(&route_targets).await?;
                return Ok(());
            }
        }

        tokio::fs::write(&conf_path, &conf).await?;
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&conf_path, std::fs::Permissions::from_mode(0o600)).await?;

        // Sync configuration using wg syncconf
        let status = Command::new("wg")
            .args(["syncconf", &self.interface, &conf_path])
            .status()
            .await?;

        if !status.success() {
            warn!("Failed to sync WG configuration for {}", self.interface);
            // Re-run init as fallback
            return self.init(private_key, host_id).await;
        }

        self.sync_routes(&route_targets).await?;

        info!(
            "WireGuard mesh updated with {} peers via wg syncconf",
            peers.len()
        );
        Ok(())
    }

    fn normalize_allowed_ips(allowed_ips: &[String]) -> Vec<String> {
        allowed_ips
            .iter()
            .map(|ip| {
                if ip.contains('/') {
                    ip.clone()
                } else if ip.contains(':') {
                    format!("{}/128", ip)
                } else {
                    format!("{}/32", ip)
                }
            })
            .collect()
    }

    async fn sync_routes(&self, route_targets: &[String]) -> anyhow::Result<()> {
        let desired: std::collections::HashSet<String> = route_targets
            .iter()
            .map(|target| Self::route_compare_key(target))
            .collect();

        let current_output = Command::new("ip")
            .args(["-6", "route", "show", "dev", &self.interface])
            .output()
            .await?;

        if current_output.status.success() {
            let current_routes: Vec<(String, String)> =
                String::from_utf8_lossy(&current_output.stdout)
                    .lines()
                    .filter_map(|line| line.split_whitespace().next())
                    .map(|target| (target.to_string(), Self::route_compare_key(target)))
                    .collect();

            for (current_target, current_key) in current_routes {
                if !desired.contains(&current_key) {
                    let status = Command::new("ip")
                        .args([
                            "-6",
                            "route",
                            "del",
                            &current_target,
                            "dev",
                            &self.interface,
                        ])
                        .status()
                        .await?;

                    if !status.success() {
                        return Err(anyhow::anyhow!(
                            "Failed to delete stale route {} on {}",
                            current_target,
                            self.interface
                        ));
                    }
                }
            }
        }

        for target in route_targets {
            let family = if target.contains(':') { "-6" } else { "-4" };
            let status = Command::new("ip")
                .args([family, "route", "replace", target, "dev", &self.interface])
                .status()
                .await?;

            if !status.success() {
                return Err(anyhow::anyhow!(
                    "Failed to install route {target} on {}",
                    self.interface
                ));
            }
        }

        Ok(())
    }

    fn route_compare_key(target: &str) -> String {
        if target.contains('/') {
            target.to_string()
        } else if target.contains(':') {
            format!("{target}/128")
        } else {
            format!("{target}/32")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WireGuardManager;

    #[test]
    fn normalize_allowed_ips_adds_prefixes_once() {
        let ips = vec![
            "fd00::1".to_string(),
            "fd00::2/128".to_string(),
            "192.168.122.10".to_string(),
            "192.168.122.11/32".to_string(),
        ];

        let normalized = WireGuardManager::normalize_allowed_ips(&ips);

        assert_eq!(
            normalized,
            vec![
                "fd00::1/128".to_string(),
                "fd00::2/128".to_string(),
                "192.168.122.10/32".to_string(),
                "192.168.122.11/32".to_string(),
            ]
        );
    }

    #[test]
    fn route_compare_key_normalizes_host_routes() {
        assert_eq!(
            WireGuardManager::route_compare_key("fd00::1"),
            "fd00::1/128"
        );
        assert_eq!(
            WireGuardManager::route_compare_key("fd00::2/128"),
            "fd00::2/128"
        );
        assert_eq!(
            WireGuardManager::route_compare_key("192.168.122.10"),
            "192.168.122.10/32"
        );
        assert_eq!(
            WireGuardManager::route_compare_key("192.168.122.0/24"),
            "192.168.122.0/24"
        );
    }
}
