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

use anyhow::Context;
use base64::Engine as _;
use futures::stream::TryStreamExt;
use netlink_packet_route::route::{RouteAddress, RouteAttribute, RouteMessage};
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

        // 2. Initialize interface with rtnetlink and wg
        info!("Initializing WireGuard interface {}", self.interface);

        let handle = Self::rtnl_handle()?;
        self.delete_link_if_exists(&handle).await?;
        self.create_wireguard_link(&handle).await?;

        let addr = host_ipv6.parse::<Ipv6Addr>()?;
        self.configure_device(private_key, self.listen_port, &[], false)
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

    fn rtnl_handle() -> anyhow::Result<rtnetlink::Handle> {
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

        let private_key_bytes = Self::decode_private_key(private_key)?;
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
                let peer_nla = Self::build_peer_attr(peer).await?;
                peers_attr = peers_attr.nest(&peer_nla)?;
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

        let pubkey = Self::normalize_public_key(&peer.wireguard_pubkey)?;
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

        let endpoint = Self::peer_endpoint_bytes(&peer.endpoint, peer.wireguard_port).await?;
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
                let allowedip = Self::build_allowed_ip_attr(ip)?;
                allowedips_attr = allowedips_attr.nest(&allowedip)?;
            }

            peer_attr = peer_attr.nest(&allowedips_attr)?;
        }

        Ok(peer_attr)
    }

    fn build_allowed_ip_attr(
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
            IpAddr::V4(_) => u16::try_from(libc::AF_INET).expect("AF_INET fits in u16"),
            IpAddr::V6(_) => u16::try_from(libc::AF_INET6).expect("AF_INET6 fits in u16"),
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
                .nla_payload(Self::ip_bytes(addr_ip))
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

    async fn peer_endpoint_bytes(host: &str, port: i32) -> anyhow::Result<Vec<u8>> {
        let port = u16::try_from(port).context("Invalid WireGuard peer port")?;
        let mut addrs = lookup_host((host, port))
            .await
            .with_context(|| format!("Failed to resolve WireGuard peer endpoint {host}:{port}"))?;
        let socket = addrs.next().ok_or_else(|| {
            anyhow::anyhow!("WireGuard peer endpoint {host}:{port} resolved to no addresses")
        })?;

        Ok(match socket {
            SocketAddr::V4(addr) => {
                let sockaddr = libc::sockaddr_in {
                    sin_family: u16::try_from(libc::AF_INET).expect("AF_INET fits in u16"),
                    sin_port: addr.port().to_be(),
                    sin_addr: libc::in_addr {
                        s_addr: u32::from(*addr.ip()).to_be(),
                    },
                    sin_zero: [0; 8],
                };
                Self::struct_bytes(&sockaddr)
            },
            SocketAddr::V6(addr) => {
                let sockaddr = libc::sockaddr_in6 {
                    sin6_family: u16::try_from(libc::AF_INET6).expect("AF_INET6 fits in u16"),
                    sin6_port: addr.port().to_be(),
                    sin6_flowinfo: addr.flowinfo(),
                    sin6_addr: libc::in6_addr {
                        s6_addr: addr.ip().octets(),
                    },
                    sin6_scope_id: addr.scope_id(),
                };
                Self::struct_bytes(&sockaddr)
            },
        })
    }

    fn decode_private_key(private_key: &str) -> anyhow::Result<Vec<u8>> {
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

    fn normalize_public_key(public_key: &str) -> anyhow::Result<Vec<u8>> {
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

    fn ip_bytes(ip: IpAddr) -> Vec<u8> {
        match ip {
            IpAddr::V4(addr) => addr.octets().to_vec(),
            IpAddr::V6(addr) => addr.octets().to_vec(),
        }
    }

    fn struct_bytes<T: Sized>(value: &T) -> Vec<u8> {
        unsafe {
            std::slice::from_raw_parts(
                std::ptr::from_ref(value).cast::<u8>(),
                std::mem::size_of::<T>(),
            )
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

        self.configure_device(private_key, self.listen_port, peers, true)
            .await?;

        self.sync_routes(&route_targets).await?;

        info!(
            "WireGuard mesh updated with {} peers via netlink",
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

        let handle = Self::rtnl_handle()?;
        let Some(index) = self.link_index(&handle).await? else {
            return Err(anyhow::anyhow!(
                "WireGuard interface {} not found",
                self.interface
            ));
        };

        let current_routes = Self::current_routes_for_interface(&handle, index).await?;
        for route in current_routes {
            if let Some(key) = Self::route_message_key(&route)
                && !desired.contains(&key)
            {
                handle.route().del(route).execute().await?;
            }
        }

        for target in route_targets {
            let (addr, prefix) = Self::parse_route_target(target)?;
            let req = handle.route().add().replace();
            match addr {
                IpAddr::V4(v4) => {
                    req.v4()
                        .destination_prefix(v4, prefix)
                        .output_interface(index)
                        .execute()
                        .await?;
                },
                IpAddr::V6(v6) => {
                    req.v6()
                        .destination_prefix(v6, prefix)
                        .output_interface(index)
                        .execute()
                        .await?;
                },
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

    fn route_message_key(route: &RouteMessage) -> Option<String> {
        let prefix = route.header.destination_prefix_length;
        route.attributes.iter().find_map(|attr| match attr {
            RouteAttribute::Destination(RouteAddress::Inet(v4)) => Some(format!("{v4}/{prefix}")),
            RouteAttribute::Destination(RouteAddress::Inet6(v6)) => Some(format!("{v6}/{prefix}")),
            _ => None,
        })
    }

    fn parse_route_target(target: &str) -> anyhow::Result<(IpAddr, u8)> {
        if let Some((addr, prefix)) = target.split_once('/') {
            let prefix = prefix.parse::<u8>()?;
            if addr.contains(':') {
                Ok((IpAddr::V6(addr.parse::<Ipv6Addr>()?), prefix))
            } else {
                Ok((IpAddr::V4(addr.parse::<Ipv4Addr>()?), prefix))
            }
        } else if target.contains(':') {
            Ok((IpAddr::V6(target.parse::<Ipv6Addr>()?), 128))
        } else {
            Ok((IpAddr::V4(target.parse::<Ipv4Addr>()?), 32))
        }
    }

    async fn current_routes_for_interface(
        handle: &rtnetlink::Handle,
        index: u32,
    ) -> anyhow::Result<Vec<RouteMessage>> {
        let v4 = handle.route().get(rtnetlink::IpVersion::V4).execute();
        let v6 = handle.route().get(rtnetlink::IpVersion::V6).execute();

        let routes_v4 = v4.try_collect::<Vec<_>>().await?;
        let routes_v6 = v6.try_collect::<Vec<_>>().await?;

        Ok(routes_v4
            .into_iter()
            .chain(routes_v6)
            .filter(|route| {
                route.attributes.iter().any(|attr| {
                    matches!(attr, RouteAttribute::Oif(route_index) if *route_index == index)
                })
            })
            .collect())
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
