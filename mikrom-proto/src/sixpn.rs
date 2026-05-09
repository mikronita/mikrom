use crate::id::UserId;
use sha2::{Digest, Sha256};
use std::net::Ipv6Addr;

pub struct SixPn;

impl SixPn {
    /// Generates a unique /40 VPC prefix for a given user identifier within the fd00::/8 range.
    /// Format: fd<32-bit-hash>::/40
    pub fn generate_vpc_prefix(user_id: UserId) -> Ipv6Addr {
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
