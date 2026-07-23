#![cfg_attr(target_arch = "bpf", no_std)]
#![cfg_attr(target_arch = "bpf", no_main)]

use aya_ebpf::{
    macros::{classifier, map},
    maps::{HashMap, PerCpuHashMap},
    programs::TcContext,
};
use mikrom_agent_ebpf_common::{FirewallRule, NetworkStats};
use network_types::{eth::EthHdr, ip::Ipv6Hdr, tcp::TcpHdr, udp::UdpHdr};

#[map]
static STATS: PerCpuHashMap<u32, NetworkStats> = PerCpuHashMap::with_max_entries(1024, 0);

#[map]
static RULES: HashMap<u32, FirewallRule> = HashMap::with_max_entries(16384, 0);

const TC_ACT_OK: i32 = 0;
const TC_ACT_SHOT: i32 = 2;

#[inline(always)]
fn increment_stats(ifindex: u32, len: u64, is_tx: bool) {
    if let Some(stats) = STATS.get_ptr_mut(ifindex) {
        unsafe {
            if is_tx {
                (*stats).tx_bytes += len;
            } else {
                (*stats).rx_bytes += len;
            }
        }
    } else {
        let initial = if is_tx {
            NetworkStats {
                tx_bytes: len,
                rx_bytes: 0,
            }
        } else {
            NetworkStats {
                tx_bytes: 0,
                rx_bytes: len,
            }
        };
        let _ = STATS.insert(ifindex, initial, 0);
    }
}

#[classifier]
pub fn mikrom_ingress(ctx: TcContext) -> i32 {
    let len = ctx.len() as u64;
    let ifindex = unsafe { (*ctx.skb.skb).ifindex };

    increment_stats(ifindex, len, true);

    TC_ACT_OK
}

#[classifier]
pub fn mikrom_egress(ctx: TcContext) -> i32 {
    let len = ctx.len() as u64;
    let ifindex = unsafe { (*ctx.skb.skb).ifindex };

    increment_stats(ifindex, len, false);

    match try_mikrom_egress(ctx, ifindex) {
        Ok(ret) => ret,
        Err(_) => TC_ACT_SHOT,
    }
}
#[inline(always)]
fn get_transport_offset_and_proto(ctx: &TcContext, ipv6hdr: &Ipv6Hdr) -> Result<(usize, u8), ()> {
    let mut next_hdr = ipv6hdr.next_hdr;
    let mut offset = EthHdr::LEN + Ipv6Hdr::LEN;

    for _ in 0..8 {
        match next_hdr {
            0 | 43 | 60 => {
                let hdr: [u8; 2] = ctx.load(offset).map_err(|_| ())?;
                next_hdr = hdr[0];
                offset += ((hdr[1] as usize) * 8) + 8;
            },
            44 => {
                let hdr: [u8; 8] = ctx.load(offset).map_err(|_| ())?;
                next_hdr = hdr[0];
                let frag_off_and_flags = ((hdr[2] as u16) << 8) | (hdr[3] as u16);
                let frag_offset = frag_off_and_flags >> 3;
                let more_frags = (frag_off_and_flags & 0x01) != 0;
                if frag_offset != 0 || more_frags {
                    return Ok((offset, 0));
                }
                offset += 8;
            },
            51 => {
                let hdr: [u8; 2] = ctx.load(offset).map_err(|_| ())?;
                next_hdr = hdr[0];
                offset += ((hdr[1] as usize) + 2) * 4;
            },
            _ => {
                return Ok((offset, next_hdr));
            },
        }
    }

    Err(())
}

fn try_mikrom_egress(ctx: TcContext, ifindex: u32) -> Result<i32, ()> {
    let ethhdr: EthHdr = ctx.load(0).map_err(|_| ())?;
    if ethhdr.ether_type != 0x86DD_u16 {
        return Ok(TC_ACT_OK);
    }

    let ipv6hdr: Ipv6Hdr = ctx.load(EthHdr::LEN).map_err(|_| ())?;
    let src_ip = ipv6hdr.src_addr;

    let (transport_offset, protocol) = get_transport_offset_and_proto(&ctx, &ipv6hdr)?;

    if protocol == 58 {
        if let Ok(icmp_type) = ctx.load::<u8>(transport_offset) {
            // Always allow ICMPv6 Neighbor Discovery Protocol (NDP) messages:
            // Router Solicitation (133), Router Advertisement (134),
            // Neighbor Solicitation (135), Neighbor Advertisement (136)
            if icmp_type >= 133 && icmp_type <= 136 {
                return Ok(TC_ACT_OK);
            }
        }
    }

    let dst_port = match protocol {
        6 => {
            let tcphdr: TcpHdr = ctx.load(transport_offset).map_err(|_| ())?;
            u16::from_be_bytes(tcphdr.dest)
        },
        17 => {
            let udphdr: UdpHdr = ctx.load(transport_offset).map_err(|_| ())?;
            u16::from_be_bytes(udphdr.dst)
        },
        _ => 0,
    };

    let mut has_rules = false;
    let mut allowed = false;

    for i in 0..16 {
        let key = (ifindex << 4) | i;
        let rule = unsafe { RULES.get(key) };
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
            if rule_proto != Protocol::Any && rule_proto as u8 != protocol {
                let is_icmp_match = (rule_proto == Protocol::Icmp
                    || rule_proto == Protocol::Icmpv6)
                    && (protocol == 1 || protocol == 58);
                if !is_icmp_match {
                    continue;
                }
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
