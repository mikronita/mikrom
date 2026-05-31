use anyhow::{Context, Result};
use std::net::{Ipv6Addr, SocketAddr};

#[derive(Clone)]
pub struct DnsConfig {
    pub listen_addr: SocketAddr,
    pub upstream_dns: Vec<SocketAddr>,
    pub allowed_subnets: Vec<ipnet::IpNet>,
    pub sys_records: Vec<(String, Ipv6Addr)>,
}

impl DnsConfig {
    pub fn from_env() -> Result<Self> {
        let upstream_dns = std::env::var("UPSTREAM_DNS")
            .ok()
            .map(|value| {
                value
                    .split(',')
                    .filter_map(|entry| parse_socket_addr(entry.trim()))
                    .collect::<Vec<_>>()
            })
            .filter(|entries| !entries.is_empty())
            .unwrap_or_else(|| {
                ["2606:4700:4700::1111", "2001:4860:4860::8888"]
                    .into_iter()
                    .filter_map(parse_socket_addr)
                    .collect()
            });

        let allowed_subnets = std::env::var("ALLOWED_SUBNETS")
            .unwrap_or_default()
            .split(',')
            .filter_map(|value| value.parse::<ipnet::IpNet>().ok())
            .collect::<Vec<_>>();

        let mut sys_records = Vec::new();
        if let Ok(ip) = std::env::var("NATS_SYS_IP")
            .unwrap_or_default()
            .parse::<Ipv6Addr>()
        {
            sys_records.push(("nats".to_string(), ip));
        }
        if let Ok(ip) = std::env::var("API_SYS_IP")
            .unwrap_or_default()
            .parse::<Ipv6Addr>()
        {
            sys_records.push(("api".to_string(), ip));
        }

        let listen_addr = "[::]:53"
            .parse()
            .context("Error parsing DNS listen address")?;

        Ok(Self {
            listen_addr,
            upstream_dns,
            allowed_subnets,
            sys_records,
        })
    }
}

fn parse_socket_addr(value: &str) -> Option<SocketAddr> {
    if value.is_empty() {
        return None;
    }

    value
        .parse::<SocketAddr>()
        .ok()
        .or_else(|| {
            value
                .parse::<std::net::IpAddr>()
                .ok()
                .map(|ip| SocketAddr::new(ip, 53))
        })
        .or_else(|| format!("{value}:53").parse::<SocketAddr>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiple_upstreams() {
        let parsed = "1.1.1.1:53,8.8.8.8:53"
            .split(',')
            .filter_map(|entry| parse_socket_addr(entry.trim()))
            .collect::<Vec<_>>();

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0], "1.1.1.1:53".parse().expect("valid socket addr"));
        assert_eq!(parsed[1], "8.8.8.8:53".parse().expect("valid socket addr"));
    }

    #[test]
    fn parses_upstreams_without_ports() {
        let parsed = "2606:4700:4700::1111,2001:4860:4860::8888"
            .split(',')
            .filter_map(|entry| parse_socket_addr(entry.trim()))
            .collect::<Vec<_>>();

        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed[0],
            "[2606:4700:4700::1111]:53"
                .parse()
                .expect("valid socket addr")
        );
        assert_eq!(
            parsed[1],
            "[2001:4860:4860::8888]:53"
                .parse()
                .expect("valid socket addr")
        );
    }
}
