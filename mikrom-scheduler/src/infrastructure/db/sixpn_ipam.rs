use sha2::{Digest, Sha256};
use std::net::Ipv6Addr;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SixPnIpam;

impl SixPnIpam {
    /// Generates a unique /40 VPC prefix for a given user UUID within the fd00::/8 range.
    /// Format: fd<32-bit-hash>::/40
    pub fn generate_vpc_prefix(user_id: Uuid) -> Ipv6Addr {
        let mut hasher = Sha256::new();
        hasher.update(user_id.as_bytes());
        let result = hasher.finalize();

        // Take first 4 bytes for the 32-bit VPC ID
        let vpc_id = &result[0..4];

        let mut octets = [0u8; 16];
        octets[0] = 0xfd;
        octets[1] = vpc_id[0];
        octets[2] = vpc_id[1];
        octets[3] = vpc_id[2];
        octets[4] = vpc_id[3];

        Ipv6Addr::from(octets)
    }

    /// Allocates a /64 address for a microVM within a VPC's /40 prefix.
    /// Format: fd<32-bit-vpc>:<24-bit-vm-id>::/64
    /// For this simplified version, we'll hash the job_id to get a deterministic 24-bit VM ID.
    pub fn allocate_vm_ipv6(vpc_prefix: Ipv6Addr, job_id: &str) -> Ipv6Addr {
        let mut hasher = Sha256::new();
        hasher.update(job_id.as_bytes());
        let result = hasher.finalize();

        // Take 3 bytes for the 24-bit VM ID
        let vm_id = &result[0..3];

        let mut octets = vpc_prefix.octets();
        // Byte 0-4 are VPC prefix (fd + 32 bits)
        // Byte 5-7 will be the VM ID
        octets[5] = vm_id[0];
        octets[6] = vm_id[1];
        octets[7] = vm_id[2];

        // Byte 8-15 remain 0 for the /64 prefix or we can set them to 1 for the actual host address
        octets[15] = 1;

        Ipv6Addr::from(octets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_generate_vpc_prefix() {
        let user_id = Uuid::from_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let prefix = SixPnIpam::generate_vpc_prefix(user_id);

        let octets = prefix.octets();
        assert_eq!(octets[0], 0xfd);
        // Ensure some bits are set from the hash
        assert!(octets[1..5].iter().any(|&b| b > 0));
        // Ensure the rest is zeroed for the prefix
        assert!(octets[5..].iter().all(|&b| b == 0));
    }

    #[test]
    fn test_allocate_vm_ipv6() {
        let user_id = Uuid::from_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let vpc_prefix = SixPnIpam::generate_vpc_prefix(user_id);
        let job_id = "test-job-123";

        let vm_ip = SixPnIpam::allocate_vm_ipv6(vpc_prefix, job_id);
        let octets = vm_ip.octets();

        assert_eq!(octets[0..5], vpc_prefix.octets()[0..5]);
        assert!(octets[5..8].iter().any(|&b| b > 0));
        assert_eq!(octets[15], 1);
    }

    #[test]
    fn test_deterministic_allocation() {
        let user_id = Uuid::new_v4();
        let prefix = SixPnIpam::generate_vpc_prefix(user_id);
        let job_id = "constant-job-id";

        let ip1 = SixPnIpam::allocate_vm_ipv6(prefix, job_id);
        let ip2 = SixPnIpam::allocate_vm_ipv6(prefix, job_id);

        assert_eq!(ip1, ip2);
    }
}
