use crate::cloud_hypervisor::manager::CloudHypervisorManager;
use crate::hypervisor::HypervisorError;
use futures::stream::TryStreamExt;
use mikrom_proto::id::VmId;
use std::ffi::CString;
use std::fs;

const TUNSETIFF: libc::c_ulong = 0x400454ca;
const TUNSETPERSIST: libc::c_ulong = 0x400454cb;
const TUNSETOWNER: libc::c_ulong = 0x400454cc;

impl CloudHypervisorManager {
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

    pub(crate) async fn setup_tap(&self, vm_id: &VmId) -> Result<(String, u32), HypervisorError> {
        let tap_name = format!("ch-tap-{}", &vm_id.to_string()[..8]);

        // CH typically runs as root or a specific user.
        // For now, let's create it with current process UID.
        let uid = unsafe { libc::getuid() };

        tokio::task::spawn_blocking({
            let tap_name = tap_name.clone();
            move || Self::create_tap_native(&tap_name, uid)
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

    pub(crate) async fn setup_routing(
        &self,
        ipv6_addr: Option<&str>,
    ) -> Result<(), HypervisorError> {
        let Some(addr) = ipv6_addr else {
            return Ok(());
        };

        let handle = self.rtnl_handle().await?;
        let bridge_name = "mikrom-br0";
        let Some(bridge_index) = self.get_link_index(&handle, bridge_name).await? else {
            return Err(HypervisorError::ProcessError(format!(
                "Bridge {bridge_name} not found"
            )));
        };

        // Extract prefix from IPv6 address (e.g., fd40:b90d:fc9e:cf57::1 -> fd40:b90d:fc9e:cf57::/64)
        let ip_part = addr.split('/').next().unwrap_or(addr);
        if let Ok(ip) = ip_part.parse::<std::net::Ipv6Addr>() {
            let mut segments = ip.segments();
            // Assuming /64 prefix, we zero out the last 4 segments
            segments[4] = 0;
            segments[5] = 0;
            segments[6] = 0;
            segments[7] = 0;
            let prefix = std::net::Ipv6Addr::new(
                segments[0],
                segments[1],
                segments[2],
                segments[3],
                segments[4],
                segments[5],
                segments[6],
                segments[7],
            );

            let res = handle
                .route()
                .add()
                .v6()
                .destination_prefix(prefix, 64)
                .output_interface(bridge_index)
                .execute()
                .await;

            if let Err(e) = res
                && !e.to_string().contains("File exists")
            {
                return Err(HypervisorError::ProcessError(format!(
                    "Failed to add route for {prefix}/64: {e}"
                )));
            }
        }

        Ok(())
    }
}
