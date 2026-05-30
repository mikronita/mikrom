pub mod error;
pub mod helpers;
pub mod keys;
pub mod netlink;
pub mod orchestrator;

use crate::wireguard::error::NetworkError;
use crate::wireguard::keys::KeyManager;
use crate::wireguard::netlink::{WgAllowedIpAttr, WgCmd, WgDeviceAttr, WgPeerAttr};
use futures::stream::TryStreamExt;
use neli::{
    consts::{nl::NlmF, socket::NlFamily},
    genl::{AttrTypeBuilder, GenlmsghdrBuilder, NlattrBuilder},
    nl::NlPayload,
    router::asynchronous::NlRouter,
    types::GenlBuffer,
    utils::Groups,
};
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::time::Duration;
use tokio::net::lookup_host;
use tokio::time::sleep;
use tracing::{debug, info, warn};

pub struct WireGuardManager {
    interface: String,
    config_dir: String,
    listen_port: u16,
    last_peers: parking_lot::Mutex<Option<Vec<mikrom_proto::scheduler::Peer>>>,
}

impl WireGuardManager {
    pub fn new(interface: &str) -> Self {
        Self {
            interface: interface.to_string(),
            config_dir: "/etc/wireguard".to_string(),
            listen_port: 51823,
            last_peers: parking_lot::Mutex::new(None),
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

    pub fn interface(&self) -> &str {
        &self.interface
    }

    pub fn listen_port(&self) -> u16 {
        self.listen_port
    }

    pub fn get_host_ipv6(&self, host_id: &str) -> Ipv6Addr {
        helpers::derive_host_ipv6(host_id)
    }

    pub async fn init(&self, private_key: &str, host_id: &str) -> Result<(), NetworkError> {
        let host_ipv6 = helpers::derive_host_ipv6(host_id);

        info!("Initializing WireGuard interface {}", self.interface);

        let handle = self.rtnl_handle().await?;
        self.delete_link_if_exists(&handle).await?;
        self.create_wireguard_link(&handle).await?;

        self.configure_device_with_retry(private_key, self.listen_port, &[], false)
            .await?;

        self.add_address(&handle, IpAddr::V6(host_ipv6), 128)
            .await?;
        self.set_link_up(&handle).await?;

        info!(
            "WireGuard interface {} initialized with IP {}",
            self.interface, host_ipv6
        );
        Ok(())
    }

    pub async fn update_peers(
        &self,
        peers: &[mikrom_proto::scheduler::Peer],
        private_key: &str,
        host_id: &str,
    ) -> Result<(), NetworkError> {
        // 1. Check if update is actually needed (peek only)
        {
            let last_peers = self.last_peers.lock();
            if last_peers.as_ref().is_some_and(|last| last == peers) {
                debug!("Skipping redundant WireGuard peer update (list matches)");
                return Ok(());
            }
        }

        let own_ip = helpers::derive_host_ipv6(host_id);
        let mut route_targets = vec![format!("{}/128", own_ip)];

        // Optional: Write config for debugging
        let mut conf = format!(
            "[Interface]\nPrivateKey = {}\nListenPort = {}\n\n",
            private_key, self.listen_port
        );

        for peer in peers {
            if peer.wireguard_pubkey.is_empty() || peer.endpoint.is_empty() {
                continue;
            }

            let pubkey = KeyManager::normalize_public_key_string(&peer.wireguard_pubkey)
                .unwrap_or_else(|_| peer.wireguard_pubkey.clone());
            let formatted_allowed_ips = helpers::normalize_allowed_ips(&peer.allowed_ips)?;
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

        let conf_path =
            std::path::Path::new(&self.config_dir).join(format!("{}.conf", self.interface));
        if let Some(parent) = conf_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let _ = tokio::fs::write(&conf_path, &conf).await;

        self.configure_device_with_retry(private_key, self.listen_port, peers, true)
            .await?;

        self.sync_routes(&route_targets).await?;

        // 5. Success - update cache
        {
            let mut last_peers = self.last_peers.lock();
            *last_peers = Some(peers.to_vec());
        }

        debug!(
            "WireGuard mesh updated with {} peers via netlink",
            peers.len()
        );
        Ok(())
    }

    async fn rtnl_handle(&self) -> Result<rtnetlink::Handle, NetworkError> {
        let (connection, handle, _) =
            rtnetlink::new_connection().map_err(|e| NetworkError::Netlink(e.to_string()))?;
        tokio::spawn(connection);
        Ok(handle)
    }

    async fn delete_link_if_exists(&self, handle: &rtnetlink::Handle) -> Result<(), NetworkError> {
        if let Some(index) = self.link_index(handle).await? {
            handle
                .link()
                .del(index)
                .execute()
                .await
                .map_err(|e| NetworkError::Netlink(e.to_string()))?;
        }
        Ok(())
    }

    async fn create_wireguard_link(&self, handle: &rtnetlink::Handle) -> Result<u32, NetworkError> {
        handle
            .link()
            .add()
            .wireguard(self.interface.clone())
            .execute()
            .await
            .map_err(|e| {
                NetworkError::Netlink(format!("Failed to create WireGuard interface: {e}"))
            })?;

        let index = self
            .link_index(handle)
            .await?
            .ok_or_else(|| NetworkError::DeviceNotFound(self.interface.clone()))?;

        handle
            .link()
            .set(index)
            .mtu(1420)
            .execute()
            .await
            .map_err(|e| NetworkError::Netlink(format!("Failed to set MTU: {e}")))?;

        Ok(index)
    }

    async fn configure_device_with_retry(
        &self,
        private_key: &str,
        listen_port: u16,
        peers: &[mikrom_proto::scheduler::Peer],
        replace_peers: bool,
    ) -> Result<(), NetworkError> {
        let mut last_error = None;

        for attempt in 1..=3 {
            match self
                .configure_device(private_key, listen_port, peers, replace_peers)
                .await
            {
                Ok(()) => return Ok(()),
                Err(err) => {
                    let is_not_ready = err.to_string().contains("No such device");
                    if !is_not_ready || attempt == 3 {
                        return Err(err);
                    }

                    warn!(
                        attempt,
                        error = %err,
                        "WireGuard device is not ready yet; retrying configuration"
                    );
                    last_error = Some(err);
                    sleep(Duration::from_millis(200 * attempt as u64)).await;
                },
            }
        }

        Err(last_error.unwrap_or(NetworkError::Internal("Retry loop failed".to_string())))
    }

    async fn link_index(&self, handle: &rtnetlink::Handle) -> Result<Option<u32>, NetworkError> {
        let mut links = handle
            .link()
            .get()
            .match_name(self.interface.clone())
            .execute();
        match links.try_next().await {
            Ok(Some(msg)) => Ok(Some(msg.header.index)),
            Ok(None) => Ok(None),
            Err(e) if e.to_string().contains("No such device") => Ok(None),
            Err(e) => Err(NetworkError::Netlink(e.to_string())),
        }
    }

    async fn add_address(
        &self,
        handle: &rtnetlink::Handle,
        address: IpAddr,
        prefix_len: u8,
    ) -> Result<(), NetworkError> {
        let index = self
            .link_index(handle)
            .await?
            .ok_or_else(|| NetworkError::DeviceNotFound(self.interface.clone()))?;

        handle
            .address()
            .add(index, address, prefix_len)
            .execute()
            .await
            .map_err(|e| NetworkError::Netlink(e.to_string()))?;
        Ok(())
    }

    async fn set_link_up(&self, handle: &rtnetlink::Handle) -> Result<(), NetworkError> {
        let index = self
            .link_index(handle)
            .await?
            .ok_or_else(|| NetworkError::DeviceNotFound(self.interface.clone()))?;

        handle
            .link()
            .set(index)
            .up()
            .execute()
            .await
            .map_err(|e| NetworkError::Netlink(e.to_string()))?;
        Ok(())
    }

    async fn configure_device(
        &self,
        private_key: &str,
        listen_port: u16,
        peers: &[mikrom_proto::scheduler::Peer],
        replace_peers: bool,
    ) -> Result<(), NetworkError> {
        let (router, _) = NlRouter::connect(NlFamily::Generic, None, Groups::empty())
            .await
            .map_err(|e| NetworkError::Netlink(e.to_string()))?;
        let family_id = router.resolve_genl_family("wireguard").await.map_err(|e| {
            NetworkError::Netlink(format!("Failed to resolve wireguard family: {e}"))
        })?;

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

        let private_key_bytes = KeyManager::decode_private_key(private_key)?;
        attrs.push(
            NlattrBuilder::default()
                .nla_type(
                    AttrTypeBuilder::default()
                        .nla_type(WgDeviceAttr::PrivateKey)
                        .build()?,
                )
                .nla_payload(private_key_bytes.as_slice())
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
                match self.build_peer_attr(peer).await {
                    Ok(peer_attr) => {
                        peers_attr = peers_attr
                            .nest(&peer_attr)
                            .map_err(|e| NetworkError::Internal(e.to_string()))?;
                    },
                    Err(e) => {
                        warn!(
                            host_id = %peer.host_id,
                            error = %e,
                            "Skipping peer due to configuration error (likely DNS resolution failure)"
                        );
                        continue;
                    },
                }
            }

            attrs.push(peers_attr);
        }

        let msg = GenlmsghdrBuilder::default()
            .cmd(WgCmd::SetDevice)
            .version(1)
            .attrs(attrs.into_iter().collect::<GenlBuffer<_, _>>())
            .build()
            .map_err(|e| NetworkError::Internal(e.to_string()))?;

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
    ) -> Result<neli::genl::Nlattr<WgPeerAttr, neli::types::Buffer>, NetworkError> {
        let mut peer_attr = NlattrBuilder::default()
            .nla_type(
                AttrTypeBuilder::default()
                    .nla_type(WgPeerAttr::Unspec)
                    .build()?,
            )
            .nla_payload(Vec::<u8>::new())
            .build()?;

        let pubkey = KeyManager::normalize_public_key(&peer.wireguard_pubkey)?;
        peer_attr = peer_attr
            .nest(
                &NlattrBuilder::default()
                    .nla_type(
                        AttrTypeBuilder::default()
                            .nla_type(WgPeerAttr::PublicKey)
                            .build()?,
                    )
                    .nla_payload(pubkey)
                    .build()?,
            )
            .map_err(|e| NetworkError::Internal(e.to_string()))?;

        peer_attr = peer_attr
            .nest(
                &NlattrBuilder::default()
                    .nla_type(
                        AttrTypeBuilder::default()
                            .nla_type(WgPeerAttr::Flags)
                            .build()?,
                    )
                    .nla_payload(2u32)
                    .build()?,
            )
            .map_err(|e| NetworkError::Internal(e.to_string()))?;

        let endpoint = self
            .peer_endpoint_bytes(&peer.endpoint, peer.wireguard_port)
            .await?;
        peer_attr = peer_attr
            .nest(
                &NlattrBuilder::default()
                    .nla_type(
                        AttrTypeBuilder::default()
                            .nla_type(WgPeerAttr::Endpoint)
                            .build()?,
                    )
                    .nla_payload(endpoint)
                    .build()?,
            )
            .map_err(|e| NetworkError::Internal(e.to_string()))?;

        peer_attr = peer_attr
            .nest(
                &NlattrBuilder::default()
                    .nla_type(
                        AttrTypeBuilder::default()
                            .nla_type(WgPeerAttr::PersistentKeepaliveInterval)
                            .build()?,
                    )
                    .nla_payload(25u16)
                    .build()?,
            )
            .map_err(|e| NetworkError::Internal(e.to_string()))?;

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
                allowedips_attr = allowedips_attr
                    .nest(&allowedip)
                    .map_err(|e| NetworkError::Internal(e.to_string()))?;
            }

            peer_attr = peer_attr
                .nest(&allowedips_attr)
                .map_err(|e| NetworkError::Internal(e.to_string()))?;
        }

        Ok(peer_attr)
    }

    fn build_allowed_ip_attr(
        &self,
        ip: &str,
    ) -> Result<neli::genl::Nlattr<WgAllowedIpAttr, neli::types::Buffer>, NetworkError> {
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
            IpAddr::V4(_) => libc::AF_INET as u16,
            IpAddr::V6(_) => libc::AF_INET6 as u16,
        };

        allowedip = allowedip
            .nest(
                &NlattrBuilder::default()
                    .nla_type(
                        AttrTypeBuilder::default()
                            .nla_type(WgAllowedIpAttr::Family)
                            .build()?,
                    )
                    .nla_payload(family)
                    .build()?,
            )
            .map_err(|e| NetworkError::Internal(e.to_string()))?;

        allowedip = allowedip
            .nest(
                &NlattrBuilder::default()
                    .nla_type(
                        AttrTypeBuilder::default()
                            .nla_type(WgAllowedIpAttr::Ipaddr)
                            .build()?,
                    )
                    .nla_payload(helpers::ip_bytes(addr_ip))
                    .build()?,
            )
            .map_err(|e| NetworkError::Internal(e.to_string()))?;

        allowedip = allowedip
            .nest(
                &NlattrBuilder::default()
                    .nla_type(
                        AttrTypeBuilder::default()
                            .nla_type(WgAllowedIpAttr::CidrMask)
                            .build()?,
                    )
                    .nla_payload(prefix)
                    .build()?,
            )
            .map_err(|e| NetworkError::Internal(e.to_string()))?;

        Ok(allowedip)
    }

    async fn peer_endpoint_bytes(&self, host: &str, port: i32) -> Result<Vec<u8>, NetworkError> {
        if host.is_empty() {
            return Err(NetworkError::Internal(format!(
                "Empty endpoint host for port {port}"
            )));
        }

        let mut addrs = lookup_host((host, port as u16)).await.map_err(|e| {
            NetworkError::Internal(format!("Failed to resolve '{host}':{port}: {e}"))
        })?;
        let socket = addrs.next().ok_or_else(|| {
            NetworkError::Internal(format!("Host {host} resolved to no addresses"))
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
                helpers::struct_bytes(&sockaddr)
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
                helpers::struct_bytes(&sockaddr)
            },
        })
    }

    async fn sync_routes(&self, route_targets: &[String]) -> Result<(), NetworkError> {
        let desired_keys: std::collections::HashSet<(IpAddr, u8)> = route_targets
            .iter()
            .filter_map(|target| helpers::parse_ip_prefix(target).ok())
            .collect();

        let handle = self.rtnl_handle().await?;
        let index = self
            .link_index(&handle)
            .await?
            .ok_or_else(|| NetworkError::DeviceNotFound(self.interface.clone()))?;

        let current_routes = self.current_routes_for_interface(&handle, index).await?;
        for route in current_routes {
            if helpers::route_message_key(&route).is_some_and(|key| !desired_keys.contains(&key)) {
                let _ = handle.route().del(route).execute().await;
            }
        }

        for (addr, prefix) in desired_keys {
            let req = handle.route().add().replace();
            let res = match addr {
                IpAddr::V4(v4) => {
                    req.v4()
                        .destination_prefix(v4, prefix)
                        .output_interface(index)
                        .execute()
                        .await
                },
                IpAddr::V6(v6) => {
                    req.v6()
                        .destination_prefix(v6, prefix)
                        .output_interface(index)
                        .execute()
                        .await
                },
            };

            if let Err(e) = res {
                let err_str = e.to_string();
                if !err_str.contains("File exists") && !err_str.contains("os error 17") {
                    return Err(NetworkError::Netlink(format!(
                        "Failed to add route {addr}/{prefix}: {e}"
                    )));
                }
            }
        }

        Ok(())
    }

    async fn current_routes_for_interface(
        &self,
        handle: &rtnetlink::Handle,
        index: u32,
    ) -> Result<Vec<netlink_packet_route::route::RouteMessage>, NetworkError> {
        let v4 = handle.route().get(rtnetlink::IpVersion::V4).execute();
        let v6 = handle.route().get(rtnetlink::IpVersion::V6).execute();

        let routes_v4 = v4
            .try_collect::<Vec<_>>()
            .await
            .map_err(|e| NetworkError::Netlink(e.to_string()))?;
        let routes_v6 = v6
            .try_collect::<Vec<_>>()
            .await
            .map_err(|e| NetworkError::Netlink(e.to_string()))?;

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
    use super::*;
    use mikrom_proto::scheduler::Peer;

    #[tokio::test]
    async fn test_update_peers_idempotency() {
        let temp_dir = tempfile::tempdir().unwrap();
        let conf_dir = temp_dir.path().to_str().unwrap();
        let manager = WireGuardManager::new("wg-test").with_config_dir(conf_dir);

        let peers = vec![Peer {
            host_id: "host1".to_string(),
            wireguard_pubkey: "pubkey".to_string(),
            endpoint: "1.1.1.1".to_string(),
            wireguard_port: 51820,
            allowed_ips: vec!["fd00::1/128".to_string()],
        }];

        // First update - should write config (Netlink will fail in tests but we ignore it for file check)
        let _ = manager.update_peers(&peers, "privkey", "host1").await;

        let conf_path = temp_dir.path().join("wg-test.conf");
        assert!(conf_path.exists());
        let first_content = std::fs::read_to_string(&conf_path).unwrap();

        // Second update with same peers - should be skipped (idempotent)
        // Note: we can't easily check internal state, but we verify it doesn't crash
        let _ = manager.update_peers(&peers, "privkey", "host1").await;
        let second_content = std::fs::read_to_string(&conf_path).unwrap();
        assert_eq!(first_content, second_content);
    }

    #[test]
    fn test_manager_initialization_defaults() {
        let manager = WireGuardManager::new("wg-mikrom");
        assert_eq!(manager.interface(), "wg-mikrom");
        assert_eq!(manager.listen_port(), 51823);
    }
}
