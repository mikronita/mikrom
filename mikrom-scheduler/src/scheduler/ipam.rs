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
    subnet_base: Ipv4Addr,
    allocated: HashSet<Ipv4Addr>,
    start_offset: u32,
    end_offset: u32,
}

impl Ipam {
    pub fn new(subnet: &str, start_offset: u32, end_offset: u32) -> Self {
        // En una implementación real, usaríamos una librería de red para parsear el CIDR.
        // Por ahora, si es 10.0.0.0/8, ajustamos la base.
        let subnet_base = if subnet.starts_with("10.") {
            Ipv4Addr::new(10, 0, 0, 0)
        } else {
            Ipv4Addr::new(172, 16, 1, 0)
        };

        Self {
            inner: Arc::new(Mutex::new(IpamInner {
                subnet_base,
                allocated: HashSet::new(),
                start_offset,
                end_offset,
            })),
        }
    }

    pub fn allocate(&self) -> Option<String> {
        let mut inner = self.inner.lock();
        let base_u32: u32 = u32::from(inner.subnet_base);

        for offset in inner.start_offset..=inner.end_offset {
            let candidate = Ipv4Addr::from(base_u32 + offset);
            if !inner.allocated.contains(&candidate) {
                inner.allocated.insert(candidate);
                return Some(candidate.to_string());
            }
        }
        None
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
        // Rango global 10.0.0.0/8
        // Evitamos las primeras IPs para gateways de nodos.
        Self::new("10.0.0.0/8", 100, 16777214)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipam_allocation() {
        let ipam = Ipam::new("10.0.0.0/8", 2, 5);

        assert_eq!(ipam.allocate(), Some("10.0.0.2".to_string()));
        assert_eq!(ipam.allocate(), Some("10.0.0.3".to_string()));
        assert_eq!(ipam.allocate(), Some("10.0.0.4".to_string()));
        assert_eq!(ipam.allocate(), Some("10.0.0.5".to_string()));
        assert_eq!(ipam.allocate(), None); // Exhausted
    }

    #[test]
    fn test_ipam_release() {
        let ipam = Ipam::new("10.0.0.0/8", 2, 2);

        let ip = ipam.allocate().unwrap();
        assert_eq!(ip, "10.0.0.2");
        assert_eq!(ipam.allocate(), None); // Full

        ipam.release(&ip);
        assert_eq!(ipam.allocate(), Some("10.0.0.2".to_string())); // Available again
    }

    #[test]
    fn test_ipam_default() {
        let ipam = Ipam::default();
        let ip = ipam.allocate().unwrap();
        assert!(ip.starts_with("10."));
    }
}
