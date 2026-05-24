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
use futures::stream::TryStreamExt;
use netlink_packet_route::route::RouteMessage;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::time::Duration;
use tokio::net::lookup_host;
use tokio::time::sleep;
use tracing::{debug, info};

mod helpers;

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

fn is_missing_device_error(error: &impl std::fmt::Display) -> bool {
    error.to_string().contains("No such device")
}

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
        debug!(
            "Checking whether WireGuard interface {} already exists",
            self.interface
        );
        self.delete_link_if_exists(&handle).await.with_context(|| {
            format!(
                "Failed to delete existing WireGuard link {}",
                self.interface
            )
        })?;
        debug!("Creating WireGuard interface {}", self.interface);
        self.create_wireguard_link(&handle).await.with_context(|| {
            format!(
                "Failed to create WireGuard interface {}. Ensure the wireguard kernel module is loaded and the process has CAP_NET_ADMIN.",
                self.interface
            )
        })?;

        debug!("Configuring WireGuard device {}", self.interface);
        self.configure_device_with_retry(private_key, self.listen_port, &[], false)
            .await?;
        debug!("Adding IPv6 address {} to {}", host_ipv6, self.interface);
        self.add_address(&handle, IpAddr::V6(host_ipv6), 128)
            .await
            .with_context(|| format!("Failed to add IPv6 address to {}", self.interface))?;
        debug!("Bringing WireGuard interface {} up", self.interface);
        self.set_link_up(&handle).await.with_context(|| {
            format!("Failed to bring WireGuard interface {} up", self.interface)
        })?;

        info!(
            "WireGuard interface {} initialized with IP {}",
            self.interface, host_ipv6
        );
        Ok(())
    }

    pub fn get_host_ipv6(&self, host_id: &str) -> Ipv6Addr {
        helpers::derive_host_ipv6(host_id)
    }

    fn rtnl_handle() -> anyhow::Result<rtnetlink::Handle> {
        let (connection, handle, _) = rtnetlink::new_connection()?;
        tokio::spawn(connection);
        Ok(handle)
    }

    async fn delete_link_if_exists(&self, handle: &rtnetlink::Handle) -> anyhow::Result<()> {
        if let Some(index) = self.link_index(handle).await? {
            handle.link().del(index).execute().await?;
        } else {
            debug!("WireGuard interface {} does not exist yet", self.interface);
        }
        Ok(())
    }

    async fn create_wireguard_link(&self, handle: &rtnetlink::Handle) -> anyhow::Result<u32> {
        handle
            .link()
            .add()
            .wireguard(self.interface.clone())
            .execute()
            .await
            .context("WireGuard netlink refused to create the interface")?;

        let index = self.link_index(handle).await?.ok_or_else(|| {
            anyhow::anyhow!("WireGuard interface {} was not created", self.interface)
        })?;

        // Set MTU 1420 for WireGuard to avoid fragmentation issues with 1500 TAP/Ethernet
        handle
            .link()
            .set(index)
            .mtu(1420)
            .execute()
            .await
            .context("Failed to set WireGuard MTU to 1420")?;

        Ok(index)
    }

    async fn configure_device_with_retry(
        &self,
        private_key: &str,
        listen_port: u16,
        peers: &[mikrom_proto::scheduler::Peer],
        replace_peers: bool,
    ) -> anyhow::Result<()> {
        let mut last_error = None;

        for attempt in 1..=3 {
            match self
                .configure_device(private_key, listen_port, peers, replace_peers)
                .await
            {
                Ok(()) => return Ok(()),
                Err(err) => {
                    let is_not_ready = is_missing_device_error(&err);
                    if !is_not_ready || attempt == 3 {
                        return Err(err);
                    }

                    tracing::warn!(
                        attempt,
                        error = %err,
                        "WireGuard device is not ready yet; retrying configuration"
                    );
                    last_error = Some(err);
                    let delay_ms = 200_u64 * u64::try_from(attempt).unwrap_or(0);
                    sleep(Duration::from_millis(delay_ms)).await;
                },
            }
        }

        Err(last_error
            .unwrap_or_else(|| anyhow::anyhow!("WireGuard retry loop exited without an error")))
    }

    async fn link_index(&self, handle: &rtnetlink::Handle) -> anyhow::Result<Option<u32>> {
        let mut links = handle
            .link()
            .get()
            .match_name(self.interface.clone())
            .execute();
        match links.try_next().await {
            Ok(Some(msg)) => Ok(Some(msg.header.index)),
            Ok(None) => Ok(None),
            Err(e) if is_missing_device_error(&e) => Ok(None),
            Err(e) => Err(e.into()),
        }
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

        let (router, _) = NlRouter::connect(NlFamily::Generic, None, Groups::empty())
            .await
            .context("Failed to connect to netlink generic family")?;
        let family_id = router
            .resolve_genl_family("wireguard")
            .await
            .context("Failed to resolve wireguard generic netlink family")?;
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

        let private_key_bytes = helpers::decode_private_key(private_key)?;
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
            .await
            .context("Failed to send WireGuard SETDEVICE request")?;

        while let Some(res) = recv
            .next::<u16, neli::genl::Genlmsghdr<WgCmd, WgDeviceAttr>>()
            .await
        {
            res.context("WireGuard SETDEVICE returned an error")?;
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

        let pubkey = helpers::normalize_public_key(&peer.wireguard_pubkey)?;
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

        let endpoint_port =
            u16::try_from(peer.wireguard_port).context("Invalid WireGuard peer port")?;
        let endpoint = Self::peer_endpoint_bytes(&peer.endpoint, endpoint_port).await?;
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

        let (addr_ip, prefix) = helpers::parse_ip_prefix(ip)?;

        let mut allowedip = NlattrBuilder::default()
            .nla_type(
                AttrTypeBuilder::default()
                    .nla_type(WgAllowedIpAttr::Unspec)
                    .build()?,
            )
            .nla_payload(Vec::<u8>::new())
            .build()?;

        let family = match addr_ip {
            IpAddr::V4(_) => u16::try_from(libc::AF_INET).unwrap_or(0),
            IpAddr::V6(_) => u16::try_from(libc::AF_INET6).unwrap_or(0),
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
                .nla_payload(helpers::ip_bytes(addr_ip))
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

    async fn peer_endpoint_bytes(host: &str, port: u16) -> anyhow::Result<Vec<u8>> {
        let mut addrs = lookup_host((host, port))
            .await
            .with_context(|| format!("Failed to resolve WireGuard peer endpoint {host}:{port}"))?;
        let socket = addrs.next().ok_or_else(|| {
            anyhow::anyhow!("WireGuard peer endpoint {host}:{port} resolved to no addresses")
        })?;

        Ok(match socket {
            SocketAddr::V4(addr) => {
                let sockaddr = libc::sockaddr_in {
                    sin_family: u16::try_from(libc::AF_INET).unwrap_or(0),
                    sin_port: addr.port().to_be(),
                    sin_addr: libc::in_addr {
                        s_addr: u32::from(*addr.ip()).to_be(),
                    },
                    sin_zero: [0; 8],
                };
                helpers::struct_bytes(&sockaddr)
            },
            SocketAddr::V6(addr) => {
                let sockaddr = libc::sockaddr_in6 {
                    sin6_family: u16::try_from(libc::AF_INET6).unwrap_or(0),
                    sin6_port: addr.port().to_be(),
                    sin6_flowinfo: addr.flowinfo(),
                    sin6_addr: libc::in6_addr {
                        s6_addr: addr.ip().octets(),
                    },
                    sin6_scope_id: addr.scope_id(),
                };
                helpers::struct_bytes(&sockaddr)
            },
        })
    }

    pub async fn update_peers(
        &self,
        peers: &[mikrom_proto::scheduler::Peer],
        private_key: &str,
        host_id: &str,
    ) -> anyhow::Result<()> {
        let own_ip = self.get_host_ipv6(host_id);
        let plan = helpers::build_wireguard_config(private_key, self.listen_port, peers, own_ip)?;

        let conf_path = format!("{}/{}.conf", self.config_dir, self.interface);

        // Write the config file for debugging/persistence, but don't skip sync
        // because the interface might have been recreated in the kernel.
        tokio::fs::write(&conf_path, &plan.rendered_config).await?;
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&conf_path, std::fs::Permissions::from_mode(0o600)).await?;

        self.configure_device_with_retry(private_key, self.listen_port, peers, true)
            .await?;

        self.sync_routes(&plan.route_targets).await?;

        debug!(
            "WireGuard mesh updated with {} peers via netlink",
            peers.len()
        );
        Ok(())
    }

    async fn sync_routes(&self, route_targets: &[String]) -> anyhow::Result<()> {
        let desired_keys: std::collections::HashSet<(IpAddr, u8)> = route_targets
            .iter()
            .filter_map(|target| helpers::parse_route_target(target).ok())
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
            if let Some(key) = helpers::route_message_key(&route) {
                if !desired_keys.contains(&key) {
                    let _ = handle.route().del(route).execute().await;
                }
            }
        }

        // Use a temporary HashSet to ensure we only try to add each unique route once
        // within this sync cycle, avoiding redundant netlink calls and EEXIST potential.
        for (addr, prefix) in &desired_keys {
            let req = handle.route().add().replace();
            let res = match addr {
                IpAddr::V4(v4) => {
                    req.v4()
                        .destination_prefix(*v4, *prefix)
                        .output_interface(index)
                        .execute()
                        .await
                },
                IpAddr::V6(v6) => {
                    req.v6()
                        .destination_prefix(*v6, *prefix)
                        .output_interface(index)
                        .execute()
                        .await
                },
            };

            if let Err(e) = res {
                // Ignore "File exists" (error 17) as we used replace, but some kernel versions
                // or conflicting route attributes might still return it.
                let err_str = e.to_string();
                let is_exists = err_str.contains("File exists") || err_str.contains("os error 17");

                if !is_exists {
                    return Err(anyhow::anyhow!(
                        "Failed to add route {}/{}: {}",
                        addr,
                        prefix,
                        e
                    ));
                }
            }
        }

        Ok(())
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
                    matches!(attr, netlink_packet_route::route::RouteAttribute::Oif(route_index) if *route_index == index)
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::WireGuardManager;
    use super::helpers;
    use super::is_missing_device_error;
    use std::io;

    #[test]
    fn normalize_allowed_ips_adds_prefixes_once() {
        let ips = vec![
            "fd00::1".to_string(),
            "fd00::2/128".to_string(),
            "192.168.122.10".to_string(),
            "192.168.122.11/32".to_string(),
        ];

        let normalized = helpers::normalize_allowed_ips(&ips).unwrap();

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
    fn parse_ip_prefix_handles_ipv4_and_ipv6_targets() {
        assert_eq!(
            helpers::parse_ip_prefix("192.168.122.10").unwrap(),
            (
                std::net::IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 122, 10)),
                32
            )
        );
        assert_eq!(
            helpers::parse_ip_prefix("fd00::1/128").unwrap(),
            (std::net::IpAddr::V6("fd00::1".parse().unwrap()), 128)
        );
    }

    #[test]
    fn missing_device_errors_are_treated_as_absent() {
        let err = io::Error::from_raw_os_error(19);
        assert!(is_missing_device_error(&err));
    }

    #[test]
    fn unrelated_errors_are_not_treated_as_absent() {
        let err = io::Error::from_raw_os_error(2);
        assert!(!is_missing_device_error(&err));
    }

    #[test]
    fn host_ipv6_derivation_is_stable() {
        let manager = WireGuardManager::new("wg0");
        assert_eq!(
            manager.get_host_ipv6("router-1").to_string(),
            manager.get_host_ipv6("router-1").to_string()
        );
    }
}
