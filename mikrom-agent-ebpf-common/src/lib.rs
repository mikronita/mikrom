#![no_std]

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Protocol {
    Any = 0,
    Tcp = 6,
    Udp = 17,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Action {
    Deny = 0,
    Allow = 1,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct NetworkStats {
    pub tx_bytes: u64,
    pub rx_bytes: u64,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct FirewallRule {
    pub protocol: Protocol,
    pub port_start: u16,
    pub port_end: u16,
    pub action: Action,
    pub remote_ip: [u8; 16], // IPv6 address, all zeros for any
    pub remote_prefix: u8,   // 0 to 128
}

#[cfg(feature = "user")]
unsafe impl aya::Pod for NetworkStats {}
#[cfg(feature = "user")]
unsafe impl aya::Pod for FirewallRule {}
#[cfg(feature = "user")]
unsafe impl aya::Pod for Protocol {}
#[cfg(feature = "user")]
unsafe impl aya::Pod for Action {}
