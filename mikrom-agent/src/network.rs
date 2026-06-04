use crate::hypervisor::HypervisorError;
use futures::TryStreamExt;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::OnceLock;
use tokio::process::Command;
use tokio::sync::Mutex;

const BRIDGE_NAME: &str = "mikrom-br0";
const BRIDGE_CIDR: &str = "fd00::1/64";
const NAT64_INTERFACE_NAME: &str = "tundra";
const NAT64_TRANSLATOR_IPV4: &str = "192.168.64.2";
const NAT64_TRANSLATOR_IPV6: &str = "fd00:6464::2";
const NAT64_ROUTER_IPV4: &str = "192.168.64.1";
const NAT64_ROUTER_IPV6: &str = "fd00:6464::1";
const NAT64_INTERFACE_IPV4: &str = "192.168.64.254/24";
const NAT64_INTERFACE_IPV6: &str = "fd00:6464::fffe/64";
const NAT64_PREFIX: &str = "64:ff9b::/96";
const NAT64_CONFIG_DIR: &str = "/var/lib/mikrom-agent/nat64";
const NAT64_CONFIG_FILE: &str = "tundra-nat64.conf";

#[derive(Debug)]
struct Nat64Runtime {
    child: tokio::process::Child,
    config_path: PathBuf,
}

static NAT64_RUNTIME: OnceLock<Mutex<Option<Nat64Runtime>>> = OnceLock::new();

pub(crate) async fn ensure_bridge() -> Result<(), HypervisorError> {
    let handle = rtnl_handle().await?;

    if get_link_index(&handle, BRIDGE_NAME).await?.is_none() {
        handle
            .link()
            .add()
            .bridge(BRIDGE_NAME.to_string())
            .execute()
            .await
            .map_err(|e| {
                HypervisorError::ProcessError(format!("Failed to create bridge {BRIDGE_NAME}: {e}"))
            })?;
    }

    let Some(index) = get_link_index(&handle, BRIDGE_NAME).await? else {
        return Err(HypervisorError::ProcessError(format!(
            "Failed to find bridge {BRIDGE_NAME}"
        )));
    };

    let (addr, prefix) = parse_ip_cidr(BRIDGE_CIDR)?;
    set_link_up(&handle, index).await?;
    set_link_mtu(&handle, index, 1420).await?;

    // Always try to add addresses, ignore "File exists" errors
    let _ = add_ip_address(&handle, index, addr, prefix).await;
    let _ = add_ip_address(&handle, index, IpAddr::V6("fe80::1".parse().unwrap()), 64).await;

    set_proc_sysctl("net/ipv6/conf/all/forwarding", "1").await?;
    set_proc_sysctl("net/ipv6/conf/default/forwarding", "1").await?;
    set_proc_sysctl(&format!("net/ipv6/conf/{BRIDGE_NAME}/forwarding"), "1").await?;

    ensure_ip6tables_nat_rule(&[
        "-t",
        "nat",
        "-s",
        "fd00::/64",
        "!",
        "-o",
        BRIDGE_NAME,
        "-j",
        "MASQUERADE",
    ])
    .await?;

    Ok(())
}

pub(crate) async fn ensure_host_networking() -> Result<(), HypervisorError> {
    ensure_bridge().await?;
    ensure_nat64().await?;
    Ok(())
}

pub async fn cleanup_host_networking() -> Result<(), HypervisorError> {
    cleanup_nat64().await
}

async fn ensure_nat64() -> Result<(), HypervisorError> {
    let state_dir =
        std::env::var("MIKROM_NAT64_DIR").unwrap_or_else(|_| NAT64_CONFIG_DIR.to_string());
    let state_dir_path = PathBuf::from(&state_dir);
    tokio::fs::create_dir_all(&state_dir_path)
        .await
        .map_err(|e| {
            HypervisorError::ProcessError(format!(
                "Failed to create NAT64 state dir {state_dir}: {e}"
            ))
        })?;

    let config_path = state_dir_path.join(NAT64_CONFIG_FILE);
    write_nat64_config(&config_path).await?;
    ensure_nat64_interface(&config_path).await?;
    ensure_nat64_addresses_and_routes().await?;
    ensure_nat64_nat_rules().await?;
    ensure_nat64_process(&config_path).await?;

    Ok(())
}

async fn write_nat64_config(config_path: &PathBuf) -> Result<(), HypervisorError> {
    let config = format!(
        "program.translator_threads = 1\n\
program.privilege_drop_user =\n\
program.privilege_drop_group =\n\
io.mode = tun\n\
io.tun.device_path =\n\
io.tun.interface_name = {interface}\n\
io.tun.owner_user =\n\
io.tun.owner_group =\n\
io.tun.multi_queue = no\n\
addressing.mode = nat64\n\
addressing.nat64_clat.ipv4 = {translator_ipv4}\n\
addressing.nat64_clat.ipv6 = {translator_ipv6}\n\
addressing.nat64_clat_siit.prefix = 64:ff9b::\n\
addressing.nat64_clat_siit.allow_translation_of_private_ips = no\n\
router.ipv4 = {router_ipv4}\n\
router.ipv6 = {router_ipv6}\n\
router.generated_packet_ttl = 224\n\
translator.ipv4.outbound_mtu = 1500\n\
translator.ipv6.outbound_mtu = 1500\n\
translator.6to4.copy_dscp_and_ecn = yes\n\
translator.4to6.copy_dscp_and_ecn = yes\n",
        interface = NAT64_INTERFACE_NAME,
        translator_ipv4 = NAT64_TRANSLATOR_IPV4,
        translator_ipv6 = NAT64_TRANSLATOR_IPV6,
        router_ipv4 = NAT64_ROUTER_IPV4,
        router_ipv6 = NAT64_ROUTER_IPV6,
    );

    tokio::fs::write(config_path, config).await.map_err(|e| {
        HypervisorError::ProcessError(format!(
            "Failed to write NAT64 config {}: {e}",
            config_path.display()
        ))
    })
}

async fn ensure_nat64_interface(config_path: &PathBuf) -> Result<(), HypervisorError> {
    if link_exists(NAT64_INTERFACE_NAME).await? {
        return Ok(());
    }

    let mut command = Command::new("tundra-nat64");
    command.arg("--config-file").arg(config_path).arg("mktun");
    run_command(command, "Failed to create NAT64 TUN interface").await
}

async fn ensure_nat64_addresses_and_routes() -> Result<(), HypervisorError> {
    let mut bring_up = Command::new("ip");
    bring_up.args(["link", "set", "dev", NAT64_INTERFACE_NAME, "up"]);
    run_command(bring_up, "Failed to bring NAT64 interface up").await?;

    let mut ipv4_addr = Command::new("ip");
    ipv4_addr.args([
        "addr",
        "replace",
        NAT64_INTERFACE_IPV4,
        "dev",
        NAT64_INTERFACE_NAME,
    ]);
    run_command(ipv4_addr, "Failed to configure NAT64 IPv4 address").await?;

    let mut ipv6_addr = Command::new("ip");
    ipv6_addr.args([
        "-6",
        "addr",
        "replace",
        NAT64_INTERFACE_IPV6,
        "dev",
        NAT64_INTERFACE_NAME,
    ]);
    run_command(ipv6_addr, "Failed to configure NAT64 IPv6 address").await?;

    let mut route = Command::new("ip");
    route.args([
        "-6",
        "route",
        "replace",
        NAT64_PREFIX,
        "dev",
        NAT64_INTERFACE_NAME,
    ]);
    run_command(route, "Failed to configure NAT64 route").await?;
    set_proc_sysctl("net/ipv4/ip_forward", "1").await?;
    set_proc_sysctl("net/ipv4/conf/all/forwarding", "1").await?;
    set_proc_sysctl("net/ipv4/conf/default/forwarding", "1").await?;
    set_proc_sysctl(
        &format!("net/ipv4/conf/{NAT64_INTERFACE_NAME}/forwarding"),
        "1",
    )
    .await
}

async fn ensure_nat64_nat_rules() -> Result<(), HypervisorError> {
    ensure_ip6tables_nat_rule_at_start(&[
        "-d",
        NAT64_PREFIX,
        "-o",
        NAT64_INTERFACE_NAME,
        "-j",
        "SNAT",
        "--to-source",
        NAT64_TRANSLATOR_IPV6,
    ])
    .await?;

    let wan_interface = detect_wan_interface().await?;
    ensure_iptables_nat_rule(&["-o", &wan_interface, "-j", "MASQUERADE"]).await
}

async fn cleanup_nat64() -> Result<(), HypervisorError> {
    cleanup_nat64_process().await?;
    cleanup_nat_rules().await?;
    Ok(())
}

async fn cleanup_nat64_process() -> Result<(), HypervisorError> {
    if let Some(runtime) = NAT64_RUNTIME.get() {
        let mut guard = runtime.lock().await;
        if let Some(mut existing) = guard.take() {
            if let Err(e) = existing.child.kill().await {
                tracing::warn!(
                    config = %existing.config_path.display(),
                    error = %e,
                    "Failed to stop NAT64 translator"
                );
            } else {
                tracing::info!(
                    config = %existing.config_path.display(),
                    "Stopped NAT64 translator"
                );
            }
        }
    }

    let mut kill = Command::new("pkill");
    kill.args(["-x", "tundra-nat64"]);
    let _ = kill.output().await;
    Ok(())
}

async fn cleanup_nat_rules() -> Result<(), HypervisorError> {
    delete_all_nat_rules(
        "ip6tables",
        "POSTROUTING",
        &[
            "-d",
            NAT64_PREFIX,
            "-o",
            NAT64_INTERFACE_NAME,
            "-j",
            "SNAT",
            "--to-source",
            NAT64_TRANSLATOR_IPV6,
        ],
    )
    .await?;
    delete_all_nat_rules(
        "ip6tables",
        "POSTROUTING",
        &[
            "-s",
            "fd00::/64",
            "!",
            "-o",
            BRIDGE_NAME,
            "-j",
            "MASQUERADE",
        ],
    )
    .await?;

    if let Ok(wan_interface) = detect_wan_interface().await {
        delete_all_nat_rules(
            "iptables",
            "POSTROUTING",
            &["-o", &wan_interface, "-j", "MASQUERADE"],
        )
        .await?;
    }

    Ok(())
}

async fn ensure_nat64_process(config_path: &PathBuf) -> Result<(), HypervisorError> {
    let runtime = NAT64_RUNTIME.get_or_init(|| Mutex::new(None));
    let mut guard = runtime.lock().await;

    if let Some(existing) = guard.as_mut() {
        match existing.child.try_wait() {
            Ok(None) => return Ok(()),
            Ok(Some(status)) => {
                tracing::warn!(
                    config = %existing.config_path.display(),
                    status = ?status,
                    "Existing NAT64 process exited; restarting"
                );
            },
            Err(e) => {
                tracing::warn!(
                    config = %existing.config_path.display(),
                    error = %e,
                    "Failed to inspect NAT64 process; restarting"
                );
            },
        }
    }

    let mut command = Command::new("tundra-nat64");
    command
        .arg("--config-file")
        .arg(config_path)
        .arg("translate")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    let child = command.spawn().map_err(|e| {
        HypervisorError::ProcessError(format!(
            "Failed to spawn NAT64 translator {}: {e}",
            config_path.display()
        ))
    })?;

    tracing::info!(config = %config_path.display(), "Started NAT64 translator");
    *guard = Some(Nat64Runtime {
        child,
        config_path: config_path.clone(),
    });
    Ok(())
}

async fn detect_wan_interface() -> Result<String, HypervisorError> {
    if let Some(iface) = std::env::var("MIKROM_WAN_INTERFACE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return Ok(iface);
    }

    if let Some(iface) = std::env::var("WAN_INTERFACE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return Ok(iface);
    }

    let output = Command::new("ip")
        .args(["-o", "route", "show", "default"])
        .output()
        .await
        .map_err(|e| {
            HypervisorError::ProcessError(format!("Failed to inspect default route: {e}"))
        })?;

    if !output.status.success() {
        return Err(HypervisorError::ProcessError(format!(
            "Failed to inspect default route: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if let Some(position) = parts.iter().position(|entry| *entry == "dev")
            && let Some(iface) = parts.get(position + 1)
        {
            return Ok((*iface).to_string());
        }
    }

    Err(HypervisorError::ProcessError(
        "Unable to detect WAN interface from default route".to_string(),
    ))
}

async fn link_exists(name: &str) -> Result<bool, HypervisorError> {
    let output = Command::new("ip")
        .args(["link", "show", "dev", name])
        .output()
        .await
        .map_err(|e| {
            HypervisorError::ProcessError(format!("Failed to inspect link {name}: {e}"))
        })?;

    Ok(output.status.success())
}

async fn ensure_ip6tables_nat_rule_at_start(args: &[&str]) -> Result<(), HypervisorError> {
    ensure_nat_rule_at_start("ip6tables", args).await
}

async fn ensure_ip6tables_nat_rule(args: &[&str]) -> Result<(), HypervisorError> {
    ensure_nat_rule("ip6tables", args).await
}

async fn ensure_iptables_nat_rule(args: &[&str]) -> Result<(), HypervisorError> {
    ensure_nat_rule("iptables", args).await
}

async fn ensure_nat_rule(program: &str, args: &[&str]) -> Result<(), HypervisorError> {
    ensure_nat_rule_with_mode(program, false, args).await
}

async fn ensure_nat_rule_at_start(program: &str, args: &[&str]) -> Result<(), HypervisorError> {
    ensure_nat_rule_at_start_with_chain(program, "POSTROUTING", args).await
}

async fn ensure_nat_rule_at_start_with_chain(
    program: &str,
    chain: &str,
    args: &[&str],
) -> Result<(), HypervisorError> {
    ensure_nat_rule_with_mode_and_chain(program, chain, true, args).await
}

async fn ensure_nat_rule_with_mode(
    program: &str,
    insert_at_start: bool,
    args: &[&str],
) -> Result<(), HypervisorError> {
    ensure_nat_rule_with_mode_and_chain(program, "POSTROUTING", insert_at_start, args).await
}

async fn ensure_nat_rule_with_mode_and_chain(
    program: &str,
    chain: &str,
    insert_at_start: bool,
    args: &[&str],
) -> Result<(), HypervisorError> {
    let mut check = Command::new(program);
    check.args(["-t", "nat", "-C", chain]);
    check.args(args);

    match check.output().await {
        Ok(output) if output.status.success() => return Ok(()),
        Ok(_) => {},
        Err(e) => {
            tracing::warn!(program, error = %e, "Unable to check NAT rule; attempting to add it");
        },
    }

    let mut add = Command::new(program);
    if insert_at_start {
        add.args(["-t", "nat", "-I", chain, "1"]);
    } else {
        add.args(["-t", "nat", "-A", chain]);
    }
    add.args(args);
    let action = if insert_at_start {
        "insert NAT rule at start"
    } else {
        "append NAT rule"
    };
    run_command(add, &format!("Failed to {action} with {program}")).await
}

async fn delete_all_nat_rules(
    program: &str,
    chain: &str,
    args: &[&str],
) -> Result<(), HypervisorError> {
    loop {
        let mut delete = Command::new(program);
        delete.args(["-t", "nat", "-D", chain]);
        delete.args(args);

        match delete.output().await {
            Ok(output) if output.status.success() => continue,
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if stderr.contains("Bad rule") || stderr.contains("No such file") {
                    break;
                }
                if stderr.contains("No chain/target/match") {
                    break;
                }
                if stderr.contains("No such file or directory") {
                    break;
                }
                if stderr.contains("No such rule") {
                    break;
                }
                if stderr.is_empty() {
                    break;
                }
                break;
            },
            Err(_) => break,
        }
    }

    Ok(())
}

async fn run_command(mut command: Command, context: &str) -> Result<(), HypervisorError> {
    let output = command
        .output()
        .await
        .map_err(|e| HypervisorError::ProcessError(format!("{context}: {e}")))?;

    if output.status.success() {
        return Ok(());
    }

    Err(HypervisorError::ProcessError(format!(
        "{context}: {}",
        String::from_utf8_lossy(&output.stderr)
    )))
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
        Err(e) if is_missing_device_error(&e) => Ok(None),
        Err(e) => Err(HypervisorError::ProcessError(format!(
            "Failed to get link index for {name}: {e}"
        ))),
    }
}

fn is_missing_device_error(error: &impl std::fmt::Display) -> bool {
    error.to_string().contains("No such device")
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

#[cfg(test)]
mod tests {
    use super::is_missing_device_error;
    use std::io;

    #[test]
    fn missing_device_errors_are_treated_as_absent() {
        let err = io::Error::from_raw_os_error(19);
        assert!(is_missing_device_error(&err));
    }

    #[test]
    fn unrelated_errors_are_not_treated_as_absent() {
        let err = io::Error::from_raw_os_error(2);
        assert!(!is_missing_device_error(&err));
    }
}
