use parking_lot::Mutex;
use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Ipam {
    inner: Arc<Mutex<IpamInner>>,
}

#[derive(Debug)]
struct IpamInner {
    gateway: Ipv4Addr,
    base: Ipv4Addr,
    prefix: u32,
    allocated: HashSet<Ipv4Addr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allocation {
    pub ip: String,
    pub gateway: String,
    pub mac: String,
}

impl Ipam {
    pub fn new(cidr: &str) -> Self {
        let (ip_str, prefix_str) = cidr.split_once('/').unwrap_or((cidr, "24"));
        let prefix: u32 = prefix_str.trim().parse().unwrap_or(24);
        let gateway: Ipv4Addr = ip_str.trim().parse().unwrap_or(Ipv4Addr::new(10, 0, 0, 1));

        let mask = if prefix == 0 {
            0u32
        } else {
            !0u32 << (32 - prefix)
        };

        let base = Ipv4Addr::from(u32::from(gateway) & mask);

        Self {
            inner: Arc::new(Mutex::new(IpamInner {
                gateway,
                base,
                prefix,
                allocated: HashSet::new(),
            })),
        }
    }

    pub fn allocate(&self) -> Option<Allocation> {
        let mut inner = self.inner.lock();
        let base_u32 = u32::from(inner.base);
        let gw_u32 = u32::from(inner.gateway);

        // Calculate max hosts based on prefix, safely handling prefix 0
        let max_hosts = if inner.prefix == 0 {
            u32::MAX - 1
        } else if inner.prefix >= 32 {
            1
        } else {
            (1u32 << (32 - inner.prefix)).saturating_sub(2)
        };

        // Safety: Limit the search range to avoid huge loops on large networks (e.g. /8)
        // We only scan up to 1024 candidates. In a real world scenario with large
        // pools, we would use a more efficient data structure than a linear scan.
        let scan_limit = std::cmp::min(max_hosts, 1024);

        for offset in 2..=scan_limit {
            let candidate = Ipv4Addr::from(base_u32 + offset);
            if u32::from(candidate) == gw_u32 {
                continue;
            }
            if !inner.allocated.contains(&candidate) {
                inner.allocated.insert(candidate);

                let o = candidate.octets();
                let mac = format!("AA:FC:{:02X}:{:02X}:{:02X}:{:02X}", o[0], o[1], o[2], o[3]);

                return Some(Allocation {
                    ip: candidate.to_string(),
                    gateway: inner.gateway.to_string(),
                    mac,
                });
            }
        }
        None
    }

    pub fn netmask(&self) -> String {
        let inner = self.inner.lock();
        let mask = if inner.prefix == 0 {
            0u32
        } else {
            !0u32 << (32 - inner.prefix)
        };
        Ipv4Addr::from(mask).to_string()
    }

    pub fn release(&self, ip_str: &str) {
        if let Ok(ip) = ip_str.parse::<Ipv4Addr>() {
            let mut inner = self.inner.lock();
            inner.allocated.remove(&ip);
        }
    }
}

impl Default for Ipam {
    fn default() -> Self {
        Self::new("10.0.0.1/8")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipam_allocation() {
        let ipam = Ipam::new("10.0.0.1/29"); // .0 to .7, .1 gw. Available: .2, .3, .4, .5, .6

        let a1 = ipam.allocate().unwrap();
        assert_eq!(a1.ip, "10.0.0.2");
        assert_eq!(a1.gateway, "10.0.0.1");
        assert_eq!(a1.mac, "AA:FC:0A:00:00:02");

        let a2 = ipam.allocate().unwrap();
        assert_eq!(a2.ip, "10.0.0.3");

        let _a3 = ipam.allocate().unwrap();
        let _a4 = ipam.allocate().unwrap();
        let a5 = ipam.allocate().unwrap();
        assert_eq!(a5.ip, "10.0.0.6");

        assert_eq!(ipam.allocate(), None); // Exhausted
    }

    #[test]
    fn test_ipam_release() {
        let ipam = Ipam::new("10.0.0.1/30"); // .0 net, .1 gw, .2 host, .3 bcast. Available: .2

        let a = ipam.allocate().unwrap();
        assert_eq!(a.ip, "10.0.0.2");
        assert_eq!(ipam.allocate(), None);

        ipam.release(&a.ip);
        let a_new = ipam.allocate().unwrap();
        assert_eq!(a_new.ip, "10.0.0.2");
    }
}
