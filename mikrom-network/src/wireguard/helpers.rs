use crate::wireguard::error::NetworkError;
use netlink_packet_route::route::{RouteAddress, RouteAttribute, RouteMessage};
use std::net::{IpAddr, Ipv6Addr};

pub fn derive_host_ipv6(host_id: &str) -> Ipv6Addr {
    let hash = blake3::hash(host_id.as_bytes());
    let bytes = hash.as_bytes();

    // Use bytes [0..8] to populate the last 64 bits of the IPv6 address
    // Prefix is fd00::/64
    let mut addr_bytes = [0u8; 16];
    addr_bytes[0] = 0xfd;
    addr_bytes[1] = 0x00;
    addr_bytes[8..16].copy_from_slice(&bytes[0..8]);

    Ipv6Addr::from(addr_bytes)
}

pub fn parse_ip_prefix(target: &str) -> Result<(IpAddr, u8), NetworkError> {
    if let Some((addr_str, prefix_str)) = target.split_once('/') {
        let prefix = prefix_str
            .parse::<u8>()
            .map_err(|_| NetworkError::InvalidIp(target.to_string()))?;
        let addr = addr_str
            .parse::<IpAddr>()
            .map_err(|_| NetworkError::InvalidIp(target.to_string()))?;
        Ok((addr, prefix))
    } else {
        let addr = target
            .parse::<IpAddr>()
            .map_err(|_| NetworkError::InvalidIp(target.to_string()))?;
        let prefix = if addr.is_ipv6() { 128 } else { 32 };
        Ok((addr, prefix))
    }
}

pub fn normalize_allowed_ips(allowed_ips: &[String]) -> Result<Vec<String>, NetworkError> {
    allowed_ips
        .iter()
        .map(|ip| parse_ip_prefix(ip).map(|(addr, prefix)| format!("{addr}/{prefix}")))
        .collect()
}

pub fn route_message_key(route: &RouteMessage) -> Option<(IpAddr, u8)> {
    let prefix = route.header.destination_prefix_length;
    route.attributes.iter().find_map(|attr| match attr {
        RouteAttribute::Destination(RouteAddress::Inet(v4)) => Some((IpAddr::V4(*v4), prefix)),
        RouteAttribute::Destination(RouteAddress::Inet6(v6)) => Some((IpAddr::V6(*v6), prefix)),
        _ => None,
    })
}

pub fn ip_bytes(ip: IpAddr) -> Vec<u8> {
    match ip {
        IpAddr::V4(addr) => addr.octets().to_vec(),
        IpAddr::V6(addr) => addr.octets().to_vec(),
    }
}

/// SAFETY: This function converts a reference to a POD struct into its byte representation.
/// It is used for sockaddr structures passed to Netlink.
pub fn struct_bytes<T: Sized>(value: &T) -> Vec<u8> {
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

    #[test]
    fn test_derive_host_ipv6_is_deterministic() {
        let host_id = "test-node-1";
        let ip1 = derive_host_ipv6(host_id);
        let ip2 = derive_host_ipv6(host_id);
        assert_eq!(ip1, ip2);
        assert!(ip1.to_string().starts_with("fd00::"));
    }

    #[test]
    fn test_normalize_allowed_ips() {
        let ips = vec!["fd00::1".to_string(), "192.168.1.1/24".to_string()];
        let normalized = normalize_allowed_ips(&ips).unwrap();
        assert_eq!(normalized[0], "fd00::1/128");
        assert_eq!(normalized[1], "192.168.1.1/24");
    }

    #[test]
    fn test_parse_ip_prefix() {
        let (ip, prefix) = parse_ip_prefix("10.0.0.1").unwrap();
        assert_eq!(ip, "10.0.0.1".parse::<IpAddr>().unwrap());
        assert_eq!(prefix, 32);

        let (ip, prefix) = parse_ip_prefix("fd00::1/64").unwrap();
        assert_eq!(ip, "fd00::1".parse::<IpAddr>().unwrap());
        assert_eq!(prefix, 64);
    }
}
