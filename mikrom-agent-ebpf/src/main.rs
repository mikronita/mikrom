#![cfg_attr(target_arch = "bpf", no_std)]
#![cfg_attr(target_arch = "bpf", no_main)]

use aya_ebpf::{
    macros::{classifier, map},
    maps::{HashMap, PerCpuHashMap},
    programs::TcContext,
};
use mikrom_agent_ebpf_common::{FirewallRule, NetworkStats};
use network_types::{
    eth::{EthHdr, EtherType},
    ip::IpProto,
    ip::Ipv6Hdr,
    tcp::TcpHdr,
    udp::UdpHdr,
};

#[map]
static STATS: PerCpuHashMap<u32, NetworkStats> = PerCpuHashMap::with_max_entries(1024, 0);

#[map]
static RULES: HashMap<u32, FirewallRule> = HashMap::with_max_entries(16384, 0);

const TC_ACT_OK: i32 = 0;
const TC_ACT_SHOT: i32 = 2;

#[classifier]
pub fn mikrom_ingress(ctx: TcContext) -> i32 {
    let len = ctx.len() as u64;
    let ifindex = unsafe { (*ctx.skb.skb).ifindex };

    if let Some(stats) = STATS.get_ptr_mut(&ifindex) {
        unsafe {
            (*stats).tx_bytes += len;
        }
    } else {
        let initial = NetworkStats {
            tx_bytes: len,
            rx_bytes: 0,
        };
        let _ = STATS.insert(&ifindex, &initial, 0);
    }

    TC_ACT_OK
}

#[classifier]
pub fn mikrom_egress(ctx: TcContext) -> i32 {
    let len = ctx.len() as u64;
    let ifindex = unsafe { (*ctx.skb.skb).ifindex };

    if let Some(stats) = STATS.get_ptr_mut(&ifindex) {
        unsafe {
            (*stats).rx_bytes += len;
        }
    } else {
        let initial = NetworkStats {
            tx_bytes: 0,
            rx_bytes: len,
        };
        let _ = STATS.insert(&ifindex, &initial, 0);
    }

    match try_mikrom_egress(ctx, ifindex) {
        Ok(ret) => ret,
        Err(_) => TC_ACT_SHOT,
    }
}

fn try_mikrom_egress(ctx: TcContext, ifindex: u32) -> Result<i32, ()> {
    let ethhdr: EthHdr = ctx.load(0).map_err(|_| ())?;
    match ethhdr.ether_type {
        EtherType::Ipv6 => {},
        _ => return Ok(TC_ACT_OK),
    }

    // TODO: Handle IPv6 extension headers (Fragment, Hop-by-Hop, etc.)
    // Current implementation only parses the fixed header.
    let ipv6hdr: Ipv6Hdr = ctx.load(EthHdr::LEN).map_err(|_| ())?;
    let src_ip = unsafe { ipv6hdr.src_addr.in6_u.u6_addr8 };
    let protocol = ipv6hdr.next_hdr;
    let dst_port = match protocol {
        IpProto::Tcp => {
            let tcphdr: TcpHdr = ctx.load(EthHdr::LEN + Ipv6Hdr::LEN).map_err(|_| ())?;
            u16::from_be(tcphdr.dest)
        },
        IpProto::Udp => {
            let udphdr: UdpHdr = ctx.load(EthHdr::LEN + Ipv6Hdr::LEN).map_err(|_| ())?;
            u16::from_be(udphdr.dest)
        },
        _ => 0,
    };

    let mut has_rules = false;
    let mut allowed = false;

    for i in 0..16 {
        let key = (ifindex << 4) | i;
        let rule = unsafe { RULES.get(&key) };
        if let Some(rule) = rule {
            has_rules = true;

            // Match IP prefix if specified
            if rule.remote_prefix != 0 {
                let mut match_ip = true;
                let full_bytes = (rule.remote_prefix / 8) as usize;
                let partial_bits = rule.remote_prefix % 8;

                for (src_byte, rule_byte) in
                    src_ip.iter().zip(rule.remote_ip.iter()).take(full_bytes)
                {
                    if src_byte != rule_byte {
                        match_ip = false;
                        break;
                    }
                }

                if match_ip && partial_bits > 0 && full_bytes < 16 {
                    let mask = !((1 << (8 - partial_bits)) - 1);
                    if (src_ip[full_bytes] & mask) != (rule.remote_ip[full_bytes] & mask) {
                        match_ip = false;
                    }
                }

                if !match_ip {
                    continue;
                }
            }

            // Match protocol
            use mikrom_agent_ebpf_common::Protocol;
            let rule_proto = rule.protocol;
            if rule_proto != Protocol::Any && rule_proto as u8 != protocol as u8 {
                continue;
            }

            // Match port
            if rule.port_start != 0 && (dst_port < rule.port_start || dst_port > rule.port_end) {
                continue;
            }

            use mikrom_agent_ebpf_common::Action;
            if rule.action == Action::Allow {
                allowed = true;
                break;
            }
        }
    }

    if has_rules && !allowed {
        return Ok(TC_ACT_SHOT);
    }

    Ok(TC_ACT_OK)
}

#[cfg(not(target_arch = "bpf"))]
fn main() {}

#[cfg(target_arch = "bpf")]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
