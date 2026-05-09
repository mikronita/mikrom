#![no_std]

#[derive(Clone, Copy)]
#[repr(C)]
pub struct NetworkStats {
    pub tx_bytes: u64,
    pub rx_bytes: u64,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct FirewallRule {
    pub protocol: u8, // 6 for TCP, 17 for UDP, 0 for any
    pub port_start: u16,
    pub port_end: u16,
    pub action: u8,          // 0 for DENY, 1 for ALLOW
    pub remote_ip: [u8; 16], // IPv6 address, all zeros for any
    pub remote_prefix: u8,   // 0 to 128
}

#[cfg(feature = "user")]
unsafe impl aya::Pod for NetworkStats {}
#[cfg(feature = "user")]
unsafe impl aya::Pod for FirewallRule {}
