use std::process::Command;
use tracing::{info, warn};

pub struct WireGuardManager {
    interface: String,
}

impl WireGuardManager {
    pub fn new(interface: &str) -> Self {
        Self {
            interface: interface.to_string(),
        }
    }

    pub fn init(&self, private_key: &str, host_id: &str) -> anyhow::Result<()> {
        let host_ipv6 = self.get_host_ipv6(host_id);

        // 1. Create baseline config
        let conf = format!(
            "[Interface]\nPrivateKey = {}\nAddress = {}/64\nListenPort = 51820\n",
            private_key, host_ipv6
        );

        let conf_path = format!("/etc/wireguard/{}.conf", self.interface);
        std::fs::create_dir_all("/etc/wireguard")?;

        use std::os::unix::fs::PermissionsExt;
        std::fs::write(&conf_path, &conf)?;
        std::fs::set_permissions(&conf_path, std::fs::Permissions::from_mode(0o600))?;

        // 2. Restart interface with wg-quick
        info!("Restarting WireGuard interface {}", self.interface);
        let _ = Command::new("wg-quick")
            .args(["down", &self.interface])
            .status();

        let output = Command::new("wg-quick")
            .args(["up", &self.interface])
            .output()?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            warn!("wg-quick up failed for {}: {}", self.interface, err);
            return Err(anyhow::anyhow!("wg-quick up failed: {}", err));
        }

        info!(
            "WireGuard interface {} initialized via wg-quick with IP {}",
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

    pub fn update_peers(
        &self,
        peers: &[mikrom_proto::scheduler::Peer],
        private_key: &str,
        host_id: &str,
    ) -> anyhow::Result<()> {
        let host_ipv6 = self.get_host_ipv6(host_id);
        let mut conf = format!(
            "[Interface]\nPrivateKey = {}\nAddress = {}/64\nListenPort = 51820\n\n",
            private_key, host_ipv6
        );

        for peer in peers {
            if peer.wireguard_pubkey.is_empty() || peer.ip_address.is_empty() {
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

            // AllowedIPs: Ensure every IP has a prefix length
            let formatted_allowed_ips: Vec<String> = peer
                .allowed_ips
                .iter()
                .map(|ip| {
                    if ip.contains('/') {
                        ip.clone()
                    } else {
                        let prefix = if ip.contains(':') { "/128" } else { "/32" };
                        format!("{}{}", ip, prefix)
                    }
                })
                .collect();

            let allowed_ips = if formatted_allowed_ips.is_empty() {
                "fd00::/8".to_string()
            } else {
                formatted_allowed_ips.join(",")
            };

            conf.push_str("[Peer]\n");
            conf.push_str(&format!("PublicKey = {}\n", pubkey));
            conf.push_str(&format!(
                "Endpoint = {}:{}\n",
                peer.ip_address, peer.wireguard_port
            ));
            conf.push_str(&format!("AllowedIPs = {}\n", allowed_ips));
            conf.push_str("PersistentKeepalive = 25\n\n");
        }

        let conf_path = format!("/etc/wireguard/{}.conf", self.interface);

        use std::os::unix::fs::PermissionsExt;
        std::fs::write(&conf_path, &conf)?;
        std::fs::set_permissions(&conf_path, std::fs::Permissions::from_mode(0o600))?;

        // Sync configuration using wg-quick strip and wg syncconf
        // This is much faster and cleaner than restarting the interface
        let sync_cmd = format!(
            "wg-quick strip {} | wg syncconf {} /dev/stdin",
            self.interface, self.interface
        );
        let output = Command::new("bash").args(["-c", &sync_cmd]).output()?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            warn!(
                "Failed to sync WG configuration for {}: {}",
                self.interface, err
            );
            // Fallback: try to bring the interface up if it was down
            let _ = Command::new("wg-quick")
                .args(["up", &self.interface])
                .status();
        }

        info!(
            "WireGuard mesh updated with {} peers via wg syncconf",
            peers.len()
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_host_ipv6_is_deterministic() {
        let manager = WireGuardManager::new("wg0");
        let host_id = "test-node-123";
        let ip1 = manager.get_host_ipv6(host_id);
        let ip2 = manager.get_host_ipv6(host_id);
        assert_eq!(ip1, ip2, "IPv6 generation must be deterministic");
        assert!(
            ip1.starts_with("fd00::"),
            "IPv6 must start with fd00:: prefix"
        );
        assert!(
            ip1.matches(':').count() >= 2,
            "IPv6 should have colons between segments"
        );
    }

    #[test]
    fn test_get_host_ipv6_unique_for_different_hosts() {
        let manager = WireGuardManager::new("wg0");
        let ip1 = manager.get_host_ipv6("host-a");
        let ip2 = manager.get_host_ipv6("host-b");
        assert_ne!(ip1, ip2, "Different host IDs should produce different IPs");
    }
}
