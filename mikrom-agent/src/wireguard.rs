use anyhow::Context;
use base64::Engine as _;
use futures::stream::TryStreamExt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use tokio::net::lookup_host;
use tracing::{debug, info};

#[neli::neli_enum(serialized_type = "u8")]
enum WgCmd {
    GetDevice = 0,
    SetDevice = 1,
}

impl neli::consts::genl::Cmd for WgCmd {}

#[neli::neli_enum(serialized_type = "u16")]
enum WgDeviceAttr {
    Unspec = 0,
    Ifindex = 1,
    Ifname = 2,
    PrivateKey = 3,
    PublicKey = 4,
    Flags = 5,
    ListenPort = 6,
    Fwmark = 7,
    Peers = 8,
}

impl neli::consts::genl::NlAttrType for WgDeviceAttr {}

#[neli::neli_enum(serialized_type = "u16")]
enum WgPeerAttr {
    Unspec = 0,
    PublicKey = 1,
    PresharedKey = 2,
    Flags = 3,
    Endpoint = 4,
    PersistentKeepaliveInterval = 5,
    LastHandshakeTime = 6,
    RxBytes = 7,
    TxBytes = 8,
    AllowedIps = 9,
    ProtocolVersion = 10,
}

impl neli::consts::genl::NlAttrType for WgPeerAttr {}

#[neli::neli_enum(serialized_type = "u16")]
enum WgAllowedIpAttr {
    Unspec = 0,
    Family = 1,
    Ipaddr = 2,
    CidrMask = 3,
    Flags = 4,
}

impl neli::consts::genl::NlAttrType for WgAllowedIpAttr {}

pub struct WireGuardManager {
    interface: String,
    config_dir: String,
}

impl WireGuardManager {
    pub fn new(interface: &str) -> Self {
        Self {
            interface: interface.to_string(),
            config_dir: "/etc/wireguard".to_string(),
        }
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

        // 2. Initialize interface with rtnetlink and wg
        info!("Initializing WireGuard interface {}", self.interface);

        let handle = self.rtnl_handle().await?;
        self.delete_link_if_exists(&handle).await?;
        self.create_wireguard_link(&handle).await?;

        let addr = host_ipv6.parse::<Ipv6Addr>()?;
        self.configure_device(private_key, 51820, &[], false)
            .await?;
        self.add_address(&handle, IpAddr::V6(addr), 64).await?;
        self.set_link_up(&handle).await?;

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

    async fn rtnl_handle(&self) -> anyhow::Result<rtnetlink::Handle> {
        let (connection, handle, _) = rtnetlink::new_connection()?;
        tokio::spawn(connection);
        Ok(handle)
    }

    async fn delete_link_if_exists(&self, handle: &rtnetlink::Handle) -> anyhow::Result<()> {
        if let Some(index) = self.link_index(handle).await? {
            handle.link().del(index).execute().await?;
        }
        Ok(())
    }

    async fn create_wireguard_link(&self, handle: &rtnetlink::Handle) -> anyhow::Result<u32> {
        handle
            .link()
            .add()
            .wireguard(self.interface.clone())
            .execute()
            .await?;
        self.link_index(handle).await?.ok_or_else(|| {
            anyhow::anyhow!("WireGuard interface {} was not created", self.interface)
        })
    }

    async fn link_index(&self, handle: &rtnetlink::Handle) -> anyhow::Result<Option<u32>> {
        let mut links = handle
            .link()
            .get()
            .match_name(self.interface.clone())
            .execute();
        Ok(links.try_next().await?.map(|msg| msg.header.index))
    }

    async fn add_address(
        &self,
        handle: &rtnetlink::Handle,
        address: IpAddr,
        prefix_len: u8,
    ) -> anyhow::Result<()> {
        let Some(index) = self.link_index(handle).await? else {
            return Err(anyhow::anyhow!(
                "WireGuard interface {} not found",
                self.interface
            ));
        };

        handle
            .address()
            .add(index, address, prefix_len)
            .execute()
            .await?;
        Ok(())
    }

    async fn set_link_up(&self, handle: &rtnetlink::Handle) -> anyhow::Result<()> {
        let Some(index) = self.link_index(handle).await? else {
            return Err(anyhow::anyhow!(
                "WireGuard interface {} not found",
                self.interface
            ));
        };

        handle.link().set(index).up().execute().await?;
        Ok(())
    }

    async fn configure_device(
        &self,
        private_key: &str,
        listen_port: u16,
        peers: &[mikrom_proto::scheduler::Peer],
        replace_peers: bool,
    ) -> anyhow::Result<()> {
        use neli::{
            consts::{nl::NlmF, socket::NlFamily},
            genl::{AttrTypeBuilder, GenlmsghdrBuilder, NlattrBuilder},
            nl::NlPayload,
            router::asynchronous::NlRouter,
            types::GenlBuffer,
            utils::Groups,
        };

        let (router, _) = NlRouter::connect(NlFamily::Generic, None, Groups::empty()).await?;
        let family_id = router.resolve_genl_family("wireguard").await?;
        let mut attrs = Vec::new();

        attrs.push(
            NlattrBuilder::default()
                .nla_type(
                    AttrTypeBuilder::default()
                        .nla_type(WgDeviceAttr::Ifname)
                        .build()?,
                )
                .nla_payload(self.interface.as_str())
                .build()?,
        );

        let private_key_bytes = self.decode_private_key(private_key)?;
        attrs.push(
            NlattrBuilder::default()
                .nla_type(
                    AttrTypeBuilder::default()
                        .nla_type(WgDeviceAttr::PrivateKey)
                        .build()?,
                )
                .nla_payload(private_key_bytes)
                .build()?,
        );

        attrs.push(
            NlattrBuilder::default()
                .nla_type(
                    AttrTypeBuilder::default()
                        .nla_type(WgDeviceAttr::ListenPort)
                        .build()?,
                )
                .nla_payload(listen_port)
                .build()?,
        );

        if replace_peers {
            attrs.push(
                NlattrBuilder::default()
                    .nla_type(
                        AttrTypeBuilder::default()
                            .nla_type(WgDeviceAttr::Flags)
                            .build()?,
                    )
                    .nla_payload(1u32)
                    .build()?,
            );
        }

        if !peers.is_empty() {
            let mut peers_attr = NlattrBuilder::default()
                .nla_type(
                    AttrTypeBuilder::default()
                        .nla_type(WgDeviceAttr::Peers)
                        .build()?,
                )
                .nla_payload(Vec::<u8>::new())
                .build()?;

            for peer in peers {
                let peer_attr = self.build_peer_attr(peer).await?;
                peers_attr = peers_attr.nest(&peer_attr)?;
            }

            attrs.push(peers_attr);
        }

        let msg = GenlmsghdrBuilder::default()
            .cmd(WgCmd::SetDevice)
            .version(1)
            .attrs(attrs.into_iter().collect::<GenlBuffer<_, _>>())
            .build()?;

        let mut recv = router
            .send::<_, _, u16, neli::genl::Genlmsghdr<WgCmd, WgDeviceAttr>>(
                family_id,
                NlmF::ACK,
                NlPayload::Payload(msg),
            )
            .await?;

        while let Some(res) = recv
            .next::<u16, neli::genl::Genlmsghdr<WgCmd, WgDeviceAttr>>()
            .await
        {
            res?;
        }

        Ok(())
    }

    async fn build_peer_attr(
        &self,
        peer: &mikrom_proto::scheduler::Peer,
    ) -> anyhow::Result<neli::genl::Nlattr<WgPeerAttr, neli::types::Buffer>> {
        use neli::genl::{AttrTypeBuilder, NlattrBuilder};

        let mut peer_attr = NlattrBuilder::default()
            .nla_type(
                AttrTypeBuilder::default()
                    .nla_type(WgPeerAttr::Unspec)
                    .build()?,
            )
            .nla_payload(Vec::<u8>::new())
            .build()?;

        let pubkey = self.normalize_public_key(&peer.wireguard_pubkey)?;
        peer_attr = peer_attr.nest(
            &NlattrBuilder::default()
                .nla_type(
                    AttrTypeBuilder::default()
                        .nla_type(WgPeerAttr::PublicKey)
                        .build()?,
                )
                .nla_payload(pubkey)
                .build()?,
        )?;

        peer_attr = peer_attr.nest(
            &NlattrBuilder::default()
                .nla_type(
                    AttrTypeBuilder::default()
                        .nla_type(WgPeerAttr::Flags)
                        .build()?,
                )
                .nla_payload(2u32)
                .build()?,
        )?;

        let endpoint = self
            .peer_endpoint_bytes(&peer.endpoint, peer.wireguard_port)
            .await?;
        peer_attr = peer_attr.nest(
            &NlattrBuilder::default()
                .nla_type(
                    AttrTypeBuilder::default()
                        .nla_type(WgPeerAttr::Endpoint)
                        .build()?,
                )
                .nla_payload(endpoint)
                .build()?,
        )?;

        peer_attr = peer_attr.nest(
            &NlattrBuilder::default()
                .nla_type(
                    AttrTypeBuilder::default()
                        .nla_type(WgPeerAttr::PersistentKeepaliveInterval)
                        .build()?,
                )
                .nla_payload(25u16)
                .build()?,
        )?;

        if !peer.allowed_ips.is_empty() {
            let mut allowedips_attr = NlattrBuilder::default()
                .nla_type(
                    AttrTypeBuilder::default()
                        .nla_type(WgPeerAttr::AllowedIps)
                        .build()?,
                )
                .nla_payload(Vec::<u8>::new())
                .build()?;

            for ip in &peer.allowed_ips {
                let allowedip = self.build_allowed_ip_attr(ip)?;
                allowedips_attr = allowedips_attr.nest(&allowedip)?;
            }

            peer_attr = peer_attr.nest(&allowedips_attr)?;
        }

        Ok(peer_attr)
    }

    fn build_allowed_ip_attr(
        &self,
        ip: &str,
    ) -> anyhow::Result<neli::genl::Nlattr<WgAllowedIpAttr, neli::types::Buffer>> {
        use neli::genl::{AttrTypeBuilder, NlattrBuilder};

        let (addr, prefix) = if let Some((addr, prefix)) = ip.split_once('/') {
            (addr, prefix.parse::<u8>()?)
        } else if ip.contains(':') {
            (ip, 128)
        } else {
            (ip, 32)
        };

        let addr_ip = if addr.contains(':') {
            IpAddr::V6(addr.parse::<Ipv6Addr>()?)
        } else {
            IpAddr::V4(addr.parse::<Ipv4Addr>()?)
        };

        let mut allowedip = NlattrBuilder::default()
            .nla_type(
                AttrTypeBuilder::default()
                    .nla_type(WgAllowedIpAttr::Unspec)
                    .build()?,
            )
            .nla_payload(Vec::<u8>::new())
            .build()?;

        let family = match addr_ip {
            IpAddr::V4(_) => libc::AF_INET as u16,
            IpAddr::V6(_) => libc::AF_INET6 as u16,
        };

        allowedip = allowedip.nest(
            &NlattrBuilder::default()
                .nla_type(
                    AttrTypeBuilder::default()
                        .nla_type(WgAllowedIpAttr::Family)
                        .build()?,
                )
                .nla_payload(family)
                .build()?,
        )?;

        allowedip = allowedip.nest(
            &NlattrBuilder::default()
                .nla_type(
                    AttrTypeBuilder::default()
                        .nla_type(WgAllowedIpAttr::Ipaddr)
                        .build()?,
                )
                .nla_payload(self.ip_bytes(addr_ip))
                .build()?,
        )?;

        allowedip = allowedip.nest(
            &NlattrBuilder::default()
                .nla_type(
                    AttrTypeBuilder::default()
                        .nla_type(WgAllowedIpAttr::CidrMask)
                        .build()?,
                )
                .nla_payload(prefix)
                .build()?,
        )?;

        Ok(allowedip)
    }

    async fn peer_endpoint_bytes(&self, host: &str, port: i32) -> anyhow::Result<Vec<u8>> {
        let mut addrs = lookup_host((host, port as u16))
            .await
            .with_context(|| format!("Failed to resolve WireGuard peer endpoint {host}:{port}"))?;
        let socket = addrs.next().ok_or_else(|| {
            anyhow::anyhow!("WireGuard peer endpoint {host}:{port} resolved to no addresses")
        })?;

        Ok(match socket {
            SocketAddr::V4(addr) => {
                let sockaddr = libc::sockaddr_in {
                    sin_family: libc::AF_INET as u16,
                    sin_port: addr.port().to_be(),
                    sin_addr: libc::in_addr {
                        s_addr: u32::from(*addr.ip()).to_be(),
                    },
                    sin_zero: [0; 8],
                };
                self.struct_bytes(&sockaddr)
            },
            SocketAddr::V6(addr) => {
                let sockaddr = libc::sockaddr_in6 {
                    sin6_family: libc::AF_INET6 as u16,
                    sin6_port: addr.port().to_be(),
                    sin6_flowinfo: addr.flowinfo(),
                    sin6_addr: libc::in6_addr {
                        s6_addr: addr.ip().octets(),
                    },
                    sin6_scope_id: addr.scope_id(),
                };
                self.struct_bytes(&sockaddr)
            },
        })
    }

    fn decode_private_key(&self, private_key: &str) -> anyhow::Result<Vec<u8>> {
        let key = base64::engine::general_purpose::STANDARD
            .decode(private_key.trim())
            .context("Failed to decode WireGuard private key")?;
        if key.len() != 32 {
            return Err(anyhow::anyhow!(
                "Invalid WireGuard private key length: expected 32 bytes, got {}",
                key.len()
            ));
        }
        Ok(key)
    }

    fn normalize_public_key(&self, public_key: &str) -> anyhow::Result<Vec<u8>> {
        let normalized =
            if public_key.len() == 64 && public_key.chars().all(|c| c.is_ascii_hexdigit()) {
                hex::decode(public_key).context("Failed to decode hex WireGuard public key")?
            } else {
                base64::engine::general_purpose::STANDARD
                    .decode(public_key.trim())
                    .context("Failed to decode WireGuard public key")?
            };

        if normalized.len() != 32 {
            return Err(anyhow::anyhow!(
                "Invalid WireGuard public key length: expected 32 bytes, got {}",
                normalized.len()
            ));
        }

        Ok(normalized)
    }

    fn ip_bytes(&self, ip: IpAddr) -> Vec<u8> {
        match ip {
            IpAddr::V4(addr) => addr.octets().to_vec(),
            IpAddr::V6(addr) => addr.octets().to_vec(),
        }
    }

    fn struct_bytes<T: Sized>(&self, value: &T) -> Vec<u8> {
        // SAFETY: `value` is a plain old data sockaddr struct and we copy its
        // byte representation into a Vec with the exact struct size.
        unsafe {
            std::slice::from_raw_parts((value as *const T).cast::<u8>(), std::mem::size_of::<T>())
                .to_vec()
        }
    }

    pub async fn update_peers(
        &self,
        peers: &[mikrom_proto::scheduler::Peer],
        private_key: &str,
        _host_id: &str,
    ) -> anyhow::Result<()> {
        let mut conf = format!(
            "[Interface]\nPrivateKey = {}\nListenPort = 51820\n\n",
            private_key
        );

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
            let formatted_allowed_ips: Vec<String> = peer
                .allowed_ips
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
                peer.endpoint, peer.wireguard_port
            ));
            conf.push_str(&format!("AllowedIPs = {}\n", allowed_ips));
            conf.push_str("PersistentKeepalive = 25\n\n");
        }

        let conf_path = format!("{}/{}.conf", self.config_dir, self.interface);

        // Idempotency check: if the config hasn't changed, do nothing
        if let Ok(existing_conf) = tokio::fs::read_to_string(&conf_path).await
            && existing_conf == conf
        {
            debug!(
                "WireGuard config for {} is unchanged, skipping sync",
                self.interface
            );
            return Ok(());
        }

        tokio::fs::write(&conf_path, &conf).await?;
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&conf_path, std::fs::Permissions::from_mode(0o600)).await?;

        self.configure_device(private_key, 51820, peers, true)
            .await?;

        info!(
            "WireGuard mesh updated with {} peers via netlink",
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

    #[tokio::test]
    async fn test_update_peers_idempotency() {
        let temp_dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&temp_dir).unwrap();
        let conf_dir = temp_dir.to_str().unwrap();
        let manager = WireGuardManager::new("m-test-wg").with_config_dir(conf_dir);

        let peers = vec![mikrom_proto::scheduler::Peer {
            host_id: "host1".to_string(),
            wireguard_pubkey: "pubkey".to_string(),
            endpoint: "1.1.1.1".to_string(),
            wireguard_port: 51820,
            allowed_ips: vec!["fd00::1/128".to_string()],
        }];

        // 1. First call should write the config file even if system commands fail
        // in a restricted test environment.
        let _ = manager.update_peers(&peers, "privkey", "host1").await;

        // The file should have been written
        let conf_path = format!("{}/m-test-wg.conf", conf_dir);
        assert!(std::path::Path::new(&conf_path).exists());
        let first_conf = std::fs::read_to_string(&conf_path).unwrap();
        assert!(!first_conf.is_empty());

        // 2. Second call with SAME data should leave the config unchanged.
        let _ = manager.update_peers(&peers, "privkey", "host1").await;
        let second_conf = std::fs::read_to_string(&conf_path).unwrap();
        assert_eq!(first_conf, second_conf);

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
