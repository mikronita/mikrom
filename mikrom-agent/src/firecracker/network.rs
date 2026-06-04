use crate::hypervisor::{HypervisorError, VmConfig};
use futures::stream::TryStreamExt;
use mikrom_proto::id::VmId;
use std::ffi::CString;
use std::fs;
use std::net::{IpAddr, Ipv6Addr};

use netlink_packet_route::route::{RouteAddress, RouteAttribute};

const TUNSETIFF: libc::c_ulong = 0x400454ca;
const TUNSETPERSIST: libc::c_ulong = 0x400454cb;
const TUNSETOWNER: libc::c_ulong = 0x400454cc;

impl crate::firecracker::FirecrackerManager {
    pub(crate) fn ipv6_route_prefix(ipv6: &str) -> Option<String> {
        let addr: std::net::Ipv6Addr = ipv6.parse().ok()?;
        let seg = addr.segments();
        let prefix = std::net::Ipv6Addr::new(seg[0], seg[1], seg[2], seg[3], 0, 0, 0, 0);
        Some(format!("{prefix}/64"))
    }

    pub(crate) async fn rtnl_handle(&self) -> Result<rtnetlink::Handle, HypervisorError> {
        let (connection, handle, _) = rtnetlink::new_connection().map_err(|e| {
            HypervisorError::ProcessError(format!("Failed to create netlink connection: {e}"))
        })?;
        tokio::spawn(connection);
        Ok(handle)
    }

    pub(crate) async fn get_link_index(
        &self,
        handle: &rtnetlink::Handle,
        name: &str,
    ) -> Result<Option<u32>, HypervisorError> {
        let mut links = handle.link().get().match_name(name.to_string()).execute();
        match links.try_next().await {
            Ok(Some(msg)) => Ok(Some(msg.header.index)),
            Ok(None) => Ok(None),
            Err(e) => Err(HypervisorError::ProcessError(format!(
                "Failed to get link index for {name}: {e}"
            ))),
        }
    }

    pub(crate) async fn set_link_up(
        &self,
        handle: &rtnetlink::Handle,
        index: u32,
    ) -> Result<(), HypervisorError> {
        handle.link().set(index).up().execute().await.map_err(|e| {
            HypervisorError::ProcessError(format!("Failed to set link {index} up: {e}"))
        })
    }

    pub(crate) fn parse_ip_cidr(&self, cidr: &str) -> Result<(IpAddr, u8), HypervisorError> {
        if let Some((ip_str, prefix_str)) = cidr.split_once('/') {
            let ip: IpAddr = ip_str.parse().map_err(|e| {
                HypervisorError::ProcessError(format!("Failed to parse IP address {ip_str}: {e}"))
            })?;
            let prefix: u8 = prefix_str.parse().map_err(|e| {
                HypervisorError::ProcessError(format!("Failed to parse prefix {prefix_str}: {e}"))
            })?;
            Ok((ip, prefix))
        } else {
            let ip: IpAddr = cidr.parse().map_err(|e| {
                HypervisorError::ProcessError(format!("Failed to parse IP address {cidr}: {e}"))
            })?;
            let prefix = if ip.is_ipv6() { 128 } else { 32 };
            Ok((ip, prefix))
        }
    }

    pub(crate) async fn init_network(&self) -> Result<(), HypervisorError> {
        crate::network::ensure_host_networking().await
    }

    pub(crate) async fn setup_tap(&self, vm_id: &VmId) -> Result<(String, u32), HypervisorError> {
        let tap_name = format!("m-tap-{}", &vm_id.to_string()[..8]);
        let jailer_uid = self.fc_config.jailer_uid;

        tokio::task::spawn_blocking({
            let tap_name = tap_name.clone();
            move || Self::create_tap_native(&tap_name, jailer_uid)
        })
        .await
        .map_err(|e| {
            HypervisorError::ProcessError(format!("Failed to join TAP creation task: {e}"))
        })?
        .map_err(|e| HypervisorError::ProcessError(format!("Failed to create TAP: {e}")))?;

        let handle = self.rtnl_handle().await?;
        let Some(index) = self.get_link_index(&handle, &tap_name).await? else {
            return Err(HypervisorError::ProcessError(format!(
                "TAP {tap_name} not found after native creation"
            )));
        };

        self.set_link_up(&handle, index).await?;

        let bridge_name = "mikrom-br0";
        let Some(bridge_index) = self.get_link_index(&handle, bridge_name).await? else {
            return Err(HypervisorError::ProcessError(format!(
                "Bridge {bridge_name} not found"
            )));
        };

        handle
            .link()
            .set(index)
            .controller(bridge_index)
            .mtu(1420)
            .execute()
            .await
            .map_err(|e| {
                HypervisorError::ProcessError(format!("Failed to attach TAP to bridge: {e}"))
            })?;

        Ok((tap_name, index))
    }

    pub(crate) fn create_tap_native(name: &str, uid: u32) -> Result<(), String> {
        use std::os::unix::io::AsRawFd;
        let iface_name = CString::new(name).map_err(|e| e.to_string())?;

        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/net/tun")
            .map_err(|e| format!("Failed to open /dev/net/tun: {e}"))?;

        let fd = file.as_raw_fd();

        let mut ifr: libc::ifreq = unsafe { std::mem::zeroed() };
        let name_bytes = iface_name.as_bytes();
        if name_bytes.len() >= ifr.ifr_name.len() {
            return Err("Interface name too long".to_string());
        }
        for (i, &byte) in name_bytes.iter().enumerate() {
            ifr.ifr_name[i] = byte as libc::c_char;
        }

        ifr.ifr_ifru.ifru_flags = (libc::IFF_TAP | libc::IFF_NO_PI) as i16;

        unsafe {
            if libc::ioctl(fd, TUNSETIFF, &ifr) < 0 {
                return Err(format!(
                    "TUNSETIFF failed: {}",
                    std::io::Error::last_os_error()
                ));
            }

            if libc::ioctl(fd, TUNSETOWNER, uid as libc::c_ulong) < 0 {
                return Err(format!(
                    "TUNSETOWNER failed: {}",
                    std::io::Error::last_os_error()
                ));
            }

            if libc::ioctl(fd, TUNSETPERSIST, 1) < 0 {
                return Err(format!(
                    "TUNSETPERSIST failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
        }

        Ok(())
    }

    pub(crate) async fn cleanup_tap(&self, tap_name: &str) {
        if let Ok(handle) = self.rtnl_handle().await
            && let Ok(Some(index)) = self.get_link_index(&handle, tap_name).await
        {
            let _ = handle.link().set(index).nocontroller().execute().await;
            let _ = handle.link().del(index).execute().await;
        }
    }

    pub(crate) async fn cleanup_ipv6_route(&self, ipv6: &str) {
        let Some(prefix_str) = Self::ipv6_route_prefix(ipv6) else {
            return;
        };

        if let Ok(handle) = self.rtnl_handle().await {
            let (addr, prefix) = match self.parse_ip_cidr(&prefix_str) {
                Ok(res) => res,
                Err(_) => return,
            };

            let mut routes = handle.route().get(rtnetlink::IpVersion::V6).execute();
            while let Ok(Some(route)) = routes.try_next().await {
                if route.header.destination_prefix_length == prefix {
                    let dest = route.attributes.iter().find_map(|attr| match attr {
                        RouteAttribute::Destination(RouteAddress::Inet6(v6)) => {
                            Some(IpAddr::V6(*v6))
                        },
                        _ => None,
                    });

                    if dest == Some(addr) {
                        if let Err(e) = handle.route().del(route).execute().await {
                            tracing::debug!(prefix = %prefix_str, "Failed to delete route: {e}");
                        } else {
                            tracing::info!(prefix = %prefix_str, "Removed IPv6 host route");
                            return;
                        }
                    }
                }
            }
        }

        tracing::warn!(
            prefix = %prefix_str,
            "IPv6 host route may still be present after delete"
        );
    }

    pub(crate) async fn add_ipv6_host_route(&self, config: &VmConfig) {
        let Some(ipv6) = &config.ipv6_address else {
            return;
        };
        let Some(prefix_str) = Self::ipv6_route_prefix(ipv6) else {
            tracing::warn!(ipv6 = %ipv6, "Failed to compute IPv6 route prefix");
            return;
        };
        let Ok(handle) = self.rtnl_handle().await else {
            tracing::warn!("Failed to create netlink connection for IPv6 route");
            return;
        };
        let Ok(Some(index)) = self.get_link_index(&handle, "mikrom-br0").await else {
            tracing::warn!("Failed to get mikrom-br0 link index for IPv6 route");
            return;
        };

        let (addr, prefix) = match self.parse_ip_cidr(&prefix_str) {
            Ok(res) => res,
            Err(e) => {
                tracing::warn!(prefix = %prefix_str, "Failed to parse IPv6 route prefix: {e}");
                return;
            },
        };

        let req = handle.route().add().replace();
        let res = match addr {
            IpAddr::V4(v4) => {
                req.v4()
                    .destination_prefix(v4, prefix)
                    .output_interface(index)
                    .execute()
                    .await
            },
            IpAddr::V6(v6) => {
                req.v6()
                    .destination_prefix(v6, prefix)
                    .output_interface(index)
                    .execute()
                    .await
            },
        };

        if let Err(e) = res {
            tracing::warn!(
                prefix = %prefix_str,
                "Failed to add IPv6 host route for VM: {e}"
            );
        } else {
            tracing::info!(prefix = %prefix_str, "Added IPv6 host route for VM");
        }
    }

    pub(crate) fn get_bridge_config(&self) -> (String, String) {
        let env_ip = std::env::var("BRIDGE_IP").ok();
        Self::resolve_bridge_config(env_ip)
    }

    pub(crate) fn resolve_bridge_config(env_ip: Option<String>) -> (String, String) {
        let bridge_name = "mikrom-br0";
        let bridge_ip = env_ip.unwrap_or_else(|| "fd00::1/64".to_string());
        (bridge_name.to_string(), bridge_ip)
    }

    pub(crate) fn parse_bridge_subnet(&self) -> (Ipv6Addr, Ipv6Addr, u32) {
        let (_, bridge_cidr) = self.get_bridge_config();
        let (ip_str, prefix_str) = bridge_cidr.split_once('/').unwrap_or((&bridge_cidr, "64"));
        let prefix: u32 = prefix_str.trim().parse().unwrap_or(64);
        let gateway: Ipv6Addr = ip_str
            .trim()
            .parse()
            .unwrap_or(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1));
        let base = Ipv6Addr::from(ipv6_to_u128(gateway) & !prefix_mask(prefix));
        (gateway, base, prefix)
    }

    pub(crate) async fn allocate_vm_network(&self) -> Option<(String, String, String)> {
        let (gateway, base, prefix) = self.parse_bridge_subnet();
        let base_u128 = ipv6_to_u128(base);
        let gateway_u128 = ipv6_to_u128(gateway);
        let subnet_end = base_u128 | prefix_mask(prefix);

        let mut allocated = self.allocated_ips.lock().await;
        let mut candidate = base_u128 + 2;

        if candidate == gateway_u128 {
            candidate = candidate.saturating_add(1);
        }

        while candidate <= subnet_end {
            let ip = u128_to_ipv6(candidate);
            if candidate != gateway_u128 && !allocated.contains(&ip) {
                allocated.insert(ip);
                return Some((ip.to_string(), gateway.to_string(), mac_from_ipv6(ip)));
            }
            candidate = candidate.saturating_add(1);
        }
        None
    }

    pub(crate) async fn release_vm_ip(&self, ip_str: &str) {
        if let Ok(ip) = ip_str.parse::<Ipv6Addr>() {
            self.allocated_ips.lock().await.remove(&ip);
        }
    }

    pub(crate) async fn attach_tc_best_effort(&self, tap: &str) {
        let mut ebpf = self.ebpf_manager.lock().await;
        if let Some(ebpf) = ebpf.as_mut()
            && let Err(e) = ebpf.attach_tc(tap)
        {
            tracing::warn!("Failed to attach eBPF filter to {}: {}", tap, e);
        }
    }
}

fn ipv6_to_u128(addr: Ipv6Addr) -> u128 {
    u128::from_be_bytes(addr.octets())
}

fn u128_to_ipv6(value: u128) -> Ipv6Addr {
    Ipv6Addr::from(value.to_be_bytes())
}

fn prefix_mask(prefix: u32) -> u128 {
    match prefix {
        0 => u128::MAX,
        128 => 0,
        n if n < 128 => (1u128 << (128 - n)) - 1,
        _ => 0,
    }
}

fn mac_from_ipv6(addr: Ipv6Addr) -> String {
    let hash = blake3::hash(&addr.octets());
    let bytes = hash.as_bytes();
    format!(
        "AA:FC:{:02X}:{:02X}:{:02X}:{:02X}",
        bytes[0], bytes[1], bytes[2], bytes[3]
    )
}
