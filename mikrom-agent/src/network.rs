use crate::hypervisor::HypervisorError;
use futures::TryStreamExt;
use std::net::IpAddr;

const BRIDGE_NAME: &str = "mikrom-br0";
const BRIDGE_CIDR: &str = "10.0.0.1/8";

pub(crate) async fn ensure_bridge() -> Result<(), HypervisorError> {
    let handle = rtnl_handle().await?;

    if get_link_index(&handle, BRIDGE_NAME).await?.is_some() {
        return Ok(());
    }

    handle
        .link()
        .add()
        .bridge(BRIDGE_NAME.to_string())
        .execute()
        .await
        .map_err(|e| {
            HypervisorError::ProcessError(format!("Failed to create bridge {BRIDGE_NAME}: {e}"))
        })?;

    let Some(index) = get_link_index(&handle, BRIDGE_NAME).await? else {
        return Err(HypervisorError::ProcessError(format!(
            "Failed to find bridge {BRIDGE_NAME} after creation"
        )));
    };

    let (addr, prefix) = parse_ip_cidr(BRIDGE_CIDR)?;
    set_link_up(&handle, index).await?;
    set_link_mtu(&handle, index, 1420).await?;
    add_ip_address(&handle, index, addr, prefix).await?;
    add_ip_address(&handle, index, IpAddr::V6("fd00::1".parse().unwrap()), 128).await?;
    add_ip_address(&handle, index, IpAddr::V6("fe80::1".parse().unwrap()), 64).await?;

    set_proc_sysctl("net/ipv4/ip_forward", "1").await?;
    set_proc_sysctl("net/ipv6/conf/all/forwarding", "1").await?;
    set_proc_sysctl("net/ipv6/conf/default/forwarding", "1").await?;
    set_proc_sysctl(&format!("net/ipv6/conf/{BRIDGE_NAME}/forwarding"), "1").await?;

    run_iptables(&[
        "-t",
        "nat",
        "-A",
        "POSTROUTING",
        "-s",
        "10.0.0.0/8",
        "!",
        "-o",
        BRIDGE_NAME,
        "-j",
        "MASQUERADE",
    ])
    .await;

    Ok(())
}

async fn rtnl_handle() -> Result<rtnetlink::Handle, HypervisorError> {
    let (connection, handle, _) = rtnetlink::new_connection().map_err(|e| {
        HypervisorError::ProcessError(format!("Failed to create netlink connection: {e}"))
    })?;
    tokio::spawn(connection);
    Ok(handle)
}

async fn get_link_index(
    handle: &rtnetlink::Handle,
    name: &str,
) -> Result<Option<u32>, HypervisorError> {
    let mut links = handle.link().get().match_name(name.to_string()).execute();
    match links.try_next().await {
        Ok(Some(link)) => Ok(Some(link.header.index)),
        Ok(None) => Ok(None),
        Err(e) => Err(HypervisorError::ProcessError(format!(
            "Failed to get link index for {name}: {e}"
        ))),
    }
}

async fn set_link_up(handle: &rtnetlink::Handle, index: u32) -> Result<(), HypervisorError> {
    handle
        .link()
        .set(index)
        .up()
        .execute()
        .await
        .map_err(|e| HypervisorError::ProcessError(format!("Failed to set link up: {e}")))
}

async fn set_link_mtu(
    handle: &rtnetlink::Handle,
    index: u32,
    mtu: u32,
) -> Result<(), HypervisorError> {
    handle
        .link()
        .set(index)
        .mtu(mtu)
        .execute()
        .await
        .map_err(|e| HypervisorError::ProcessError(format!("Failed to set MTU: {e}")))
}

async fn add_ip_address(
    handle: &rtnetlink::Handle,
    index: u32,
    addr: IpAddr,
    prefix: u8,
) -> Result<(), HypervisorError> {
    handle
        .address()
        .add(index, addr, prefix)
        .execute()
        .await
        .map_err(|e| HypervisorError::ProcessError(format!("Failed to add IP address: {e}")))
}

fn parse_ip_cidr(cidr: &str) -> Result<(IpAddr, u8), HypervisorError> {
    let (ip_str, prefix_str) = cidr
        .split_once('/')
        .ok_or_else(|| HypervisorError::ProcessError(format!("Invalid CIDR (no '/'): {cidr}")))?;
    let ip: IpAddr = ip_str
        .parse()
        .map_err(|e| HypervisorError::ProcessError(format!("Invalid IP in CIDR {cidr}: {e}")))?;
    let prefix: u8 = prefix_str.parse().map_err(|e| {
        HypervisorError::ProcessError(format!("Invalid prefix in CIDR {cidr}: {e}"))
    })?;
    Ok((ip, prefix))
}

async fn set_proc_sysctl(key: &str, value: &str) -> Result<(), HypervisorError> {
    let path = format!("/proc/sys/{key}");
    tokio::fs::write(&path, value)
        .await
        .map_err(|e| HypervisorError::ProcessError(format!("Failed to write {path}: {e}")))
}

async fn run_iptables(args: &[&str]) {
    let output = tokio::process::Command::new("iptables")
        .args(args)
        .output()
        .await;
    match output {
        Ok(o) if !o.status.success() => {
            tracing::warn!("iptables failed: {}", String::from_utf8_lossy(&o.stderr));
        },
        Err(e) => {
            tracing::warn!("iptables command error: {e}");
        },
        _ => {},
    }
}
