use anyhow::Context;
use base64::Engine as _;
use netlink_packet_route::route::{RouteAddress, RouteAttribute, RouteMessage};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub(super) fn decode_private_key(private_key: &str) -> anyhow::Result<Vec<u8>> {
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

pub(super) fn normalize_public_key(public_key: &str) -> anyhow::Result<Vec<u8>> {
    let normalized = if public_key.len() == 64 && public_key.chars().all(|c| c.is_ascii_hexdigit())
    {
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

pub(super) fn normalize_public_key_string(public_key: &str) -> anyhow::Result<String> {
    let normalized = normalize_public_key(public_key)?;
    use base64::Engine as _;
    Ok(base64::engine::general_purpose::STANDARD.encode(normalized))
}

pub(super) fn derive_host_ipv6(host_id: &str) -> Ipv6Addr {
    let mut hash: u64 = 5381;
    for c in host_id.bytes() {
        hash = ((hash << 5).wrapping_add(hash)) ^ (c as u64);
    }

    let s1 = u16::try_from((hash >> 16) & 0xFFFF).unwrap_or(0);
    let s2 = u16::try_from(hash & 0xFFFF).unwrap_or(0);

    Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, s1, s2)
}

pub(super) fn build_wireguard_config(
    private_key: &str,
    listen_port: u16,
    peers: &[mikrom_proto::scheduler::Peer],
    own_ip: Ipv6Addr,
) -> (String, Vec<String>) {
    let mut conf = format!(
        "[Interface]\nPrivateKey = {}\nListenPort = {}\n\n",
        private_key, listen_port
    );
    let mut route_targets = vec![format!("{}/128", own_ip)];

    for peer in peers {
        if peer.wireguard_pubkey.is_empty() || peer.endpoint.is_empty() {
            continue;
        }

        let pubkey = normalize_public_key_string(&peer.wireguard_pubkey)
            .unwrap_or_else(|_| peer.wireguard_pubkey.clone());
        let formatted_allowed_ips = normalize_allowed_ips(&peer.allowed_ips);
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

    (conf, route_targets)
}

pub(super) fn normalize_allowed_ips(allowed_ips: &[String]) -> Vec<String> {
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

pub(super) fn parse_route_target(target: &str) -> anyhow::Result<(IpAddr, u8)> {
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

pub(super) fn route_message_key(route: &RouteMessage) -> Option<(IpAddr, u8)> {
    let prefix = route.header.destination_prefix_length;
    route.attributes.iter().find_map(|attr| match attr {
        RouteAttribute::Destination(RouteAddress::Inet(v4)) => Some((IpAddr::V4(*v4), prefix)),
        RouteAttribute::Destination(RouteAddress::Inet6(v6)) => Some((IpAddr::V6(*v6), prefix)),
        _ => None,
    })
}

pub(super) fn ip_bytes(ip: IpAddr) -> Vec<u8> {
    match ip {
        IpAddr::V4(addr) => addr.octets().to_vec(),
        IpAddr::V6(addr) => addr.octets().to_vec(),
    }
}

pub(super) fn struct_bytes<T: Sized>(value: &T) -> Vec<u8> {
    unsafe {
        std::slice::from_raw_parts(
            std::ptr::from_ref(value).cast::<u8>(),
            std::mem::size_of::<T>(),
        )
        .to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mikrom_proto::scheduler::Peer;

    #[test]
    fn normalize_allowed_ips_adds_prefixes_once() {
        let ips = vec![
            "fd00::1".to_string(),
            "fd00::2/128".to_string(),
            "192.168.122.10".to_string(),
            "192.168.122.11/32".to_string(),
        ];

        let normalized = normalize_allowed_ips(&ips);

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
    fn derive_host_ipv6_is_stable() {
        assert_eq!(derive_host_ipv6("router-1"), derive_host_ipv6("router-1"));
    }

    #[test]
    fn build_wireguard_config_renders_peers_and_routes() {
        let peers = vec![Peer {
            host_id: "peer-1".to_string(),
            endpoint: "10.0.0.2".to_string(),
            wireguard_pubkey: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .to_string(),
            allowed_ips: vec!["fd00::2".to_string(), "192.168.122.10/32".to_string()],
            wireguard_port: 51820,
        }];

        let (config, routes) =
            build_wireguard_config("private-key", 51821, &peers, derive_host_ipv6("router-1"));

        assert!(config.contains("[Interface]"));
        assert!(config.contains("PrivateKey = private-key"));
        assert!(config.contains("Endpoint = 10.0.0.2:51820"));
        assert!(config.contains("AllowedIPs = fd00::2/128,192.168.122.10/32"));
        assert_eq!(routes[0], format!("{}/128", derive_host_ipv6("router-1")));
        assert!(routes.contains(&"fd00::2/128".to_string()));
        assert!(routes.contains(&"192.168.122.10/32".to_string()));
    }
}
