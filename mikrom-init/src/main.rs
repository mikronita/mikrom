use anyhow::{Context, Result, anyhow};
use nix::mount::{MsFlags, mount};
use nix::unistd::User;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;
use tokio::net::TcpStream;
use tokio::time::{Duration, Instant};

const CONFIG_PATH: &str = "/etc/mikrom/init.json";
const FALLBACK_SHELL: &str = "/bin/sh";

#[derive(Debug, Serialize, Deserialize)]
struct VolumeConfig {
    pub drive_id: String,
    pub mount_point: String,
    pub index: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
struct InitConfig {
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default = "default_workdir")]
    workdir: String,
    entrypoint: Vec<String>,
    #[serde(default)]
    cmd: Vec<String>,
    #[serde(default)]
    volumes: Vec<VolumeConfig>,
}

fn default_workdir() -> String {
    "/app".to_string()
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    println!("[mikrom-init] Initializing microVM environment...");

    if let Err(e) = setup_mounts() {
        eprintln!("[mikrom-init] Warning: Mount setup encountered errors: {e}");
    }

    if let Err(e) = setup_system().await {
        eprintln!("[mikrom-init] Warning: System setup encountered errors: {e}");
    }

    let config = match load_config(CONFIG_PATH) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("[mikrom-init] Error loading configuration: {e}");
            fallback_to_shell();
        },
    };

    if let Err(e) = setup_networking(&config).await {
        eprintln!("[mikrom-init] Warning: Networking setup encountered errors: {e}");
    }

    if let Err(e) = setup_volume_mounts(&config) {
        eprintln!("[mikrom-init] Warning: Volume mounting encountered errors: {e}");
    }

    // Start background services from the base image
    if let Err(e) = start_background_services().await {
        eprintln!("[mikrom-init] Warning: Failed to start background services: {e}");
    }

    println!(
        "[mikrom-init] Starting application: {:?}",
        config.entrypoint
    );

    let port = config
        .env
        .get("PORT")
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8080);

    let mut child = spawn_application(config)?;

    if let Err(e) = wait_for_port_ready(port, &mut child).await {
        eprintln!("[mikrom-init] Application never became ready: {e}");
        fallback_to_shell();
    }

    // Marker to let mikrom-agent know that subsequent logs are from the application
    println!("__MIKROM_APP_START__");

    match child.wait().await {
        Ok(status) => {
            eprintln!("[mikrom-init] Application exited with status: {status}");
        },
        Err(e) => {
            eprintln!("[mikrom-init] Failed while waiting for application exit: {e}");
        },
    }

    fallback_to_shell();
}

fn setup_mounts() -> Result<()> {
    // 1. Mount essential filesystems
    mount_fs("proc", "/proc", "proc", MsFlags::empty())?;
    mount_fs("sysfs", "/sys", "sysfs", MsFlags::empty())?;

    // devtmpfs might fail if not supported by kernel or already mounted
    if let Err(e) = mount_fs("devtmpfs", "/dev", "devtmpfs", MsFlags::empty()) {
        // Suppress "Device or resource busy" as it means it's already mounted
        if !e.to_string().contains("EBUSY") {
            eprintln!("[mikrom-init] Warning: Failed to mount /dev: {e}");
        }
    }

    // Create mount points and mount tmpfs
    let tmp_dirs = ["/run", "/tmp", "/dev/pts", "/dev/shm"];
    for dir in &tmp_dirs {
        let _ = fs::create_dir_all(dir);
    }

    mount_fs("tmpfs", "/run", "tmpfs", MsFlags::empty())?;
    mount_fs("tmpfs", "/tmp", "tmpfs", MsFlags::empty())?;
    mount_fs("tmpfs", "/dev/shm", "tmpfs", MsFlags::empty())?;

    if let Err(e) = mount_fs("devpts", "/dev/pts", "devpts", MsFlags::empty()) {
        eprintln!("[mikrom-init] Warning: Failed to mount /dev/pts: {e}");
    }

    Ok(())
}

use futures::stream::TryStreamExt;

async fn setup_system() -> Result<()> {
    // Set hostname
    let _ = nix::unistd::sethostname("localhost");
    ensure_etc_hosts("localhost")?;

    // Bring up loopback interface
    println!("[mikrom-init] Bringing up loopback interface...");
    let (connection, handle, _) = rtnetlink::new_connection()?;
    tokio::spawn(connection);

    let mut links = handle.link().get().match_name("lo".into()).execute();
    if let Some(msg) = links
        .try_next()
        .await
        .map_err(|e| anyhow!("Failed to get loopback link: {e}"))?
    {
        handle
            .link()
            .set(msg.header.index)
            .up()
            .execute()
            .await
            .map_err(|e| anyhow!("Failed to set loopback up: {e}"))?;
    }

    Ok(())
}

fn ensure_etc_hosts(hostname: &str) -> Result<()> {
    let etc_dir = Path::new("/etc");
    if !etc_dir.exists() {
        fs::create_dir_all(etc_dir)
            .with_context(|| "Failed to create /etc directory for system files")?;
    }

    let hosts_path = etc_dir.join("hosts");
    if !hosts_path.exists() {
        fs::write(&hosts_path, build_hosts_contents(hostname))
            .with_context(|| format!("Failed to write {}", hosts_path.display()))?;
    }

    let hostname_path = etc_dir.join("hostname");
    if !hostname_path.exists() {
        fs::write(&hostname_path, format!("{hostname}\n"))
            .with_context(|| format!("Failed to write {}", hostname_path.display()))?;
    }

    Ok(())
}

fn build_hosts_contents(hostname: &str) -> String {
    format!("127.0.0.1 localhost\n::1 localhost ip6-localhost ip6-loopback\n127.0.1.1 {hostname}\n")
}

async fn setup_networking(config: &InitConfig) -> Result<()> {
    let (connection, handle, _) = rtnetlink::new_connection()?;
    tokio::spawn(connection);

    // Auto-detect the first valid network interface via /sys/class/net
    let mut iface_name = None;
    let entries = std::fs::read_dir("/sys/class/net")
        .map_err(|e| anyhow!("Failed to read /sys/class/net: {e}"))?;

    let mut candidates = Vec::new();
    for entry in entries {
        if let Ok(entry) = entry {
            let name = entry.file_name().to_string_lossy().to_string();
            // Ignore loopback and common virtual tunnel interfaces
            if name != "lo" && name != "sit0" && name != "tunl0" && !name.starts_with("gre") {
                candidates.push(name);
            }
        }
    }

    // Sort to be deterministic and prefer eth0 or enp*
    candidates.sort();
    
    if let Some(name) = candidates.first() {
        println!("[mikrom-init] Detected network interface: {}", name);
        iface_name = Some(name.clone());
    }

    let link_name = iface_name.ok_or_else(|| anyhow!("No valid network interface found"))?;

    // Get the index of the detected link
    let mut links = handle.link().get().match_name(link_name.clone()).execute();
    let link_index = if let Some(msg) = links.try_next().await.map_err(|e| anyhow!("Failed to get {} link: {e}", link_name))? {
        msg.header.index
    } else {
        return Err(anyhow!("Interface {} not found in netlink", link_name));
    };

    println!("[mikrom-init] Configuring {} interface...", link_name);

    // Explicitly enable IPv6 for this interface via sysctl
    let disable_ipv6_path = format!("/proc/sys/net/ipv6/conf/{}/disable_ipv6", link_name);
    if Path::new(&disable_ipv6_path).exists() {
        if let Err(e) = std::fs::write(&disable_ipv6_path, "0") {
            eprintln!("[mikrom-init] Warning: Failed to enable IPv6 on {}: {}", link_name, e);
        }
    }

    // Try to bring the link up first
    if let Err(e) = handle.link().set(link_index).up().execute().await {
        eprintln!("[mikrom-init] Warning: Failed to set {} up: {}", link_name, e);
    } else {
        println!("[mikrom-init] {} is now UP", link_name);
    }

    // Try to set MTU separately (ignore errors as some drivers don't allow changing it)
    if let Err(e) = handle.link().set(link_index).mtu(1500).execute().await {
        eprintln!("[mikrom-init] Warning: Failed to set MTU on {}: {} (ignoring)", link_name, e);
    }

    if let Some(ipv6_addr_str) = config.env.get("IPV6_ADDR") {
        let (addr, prefix) = if let Some((addr_part, prefix_part)) = ipv6_addr_str.split_once('/') {
            (
                addr_part.parse::<std::net::Ipv6Addr>()?,
                prefix_part.parse::<u8>()?,
            )
        } else {
            (ipv6_addr_str.parse::<std::net::Ipv6Addr>()?, 64)
        };

        println!(
            "[mikrom-init] Configuring IPv6 address: {}/{} on {}",
            addr, prefix, link_name
        );
        handle
            .address()
            .add(link_index, std::net::IpAddr::V6(addr), prefix)
            .execute()
            .await
            .map_err(|e| anyhow!("Failed to add IPv6 address to {}: {e}", link_name))?;

        if let Some(ipv6_gw_str) = config.env.get("IPV6_GW") {
            let gw = ipv6_gw_str.parse::<std::net::Ipv6Addr>()?;
            println!("[mikrom-init] Configuring IPv6 gateway: {} via {}", gw, link_name);

            handle
                .route()
                .add()
                .v6()
                .destination_prefix(std::net::Ipv6Addr::UNSPECIFIED, 0)
                .gateway(gw)
                .output_interface(link_index)
                .execute()
                .await
                .map_err(|e| anyhow!("Failed to add IPv6 gateway: {e}"))?;
        }
    }

    Ok(())
}

fn load_config(path: &str) -> Result<InitConfig> {
    let config_str = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file at {path}"))?;

    serde_json::from_str(&config_str).with_context(|| "Failed to parse configuration JSON")
}

fn build_command(config: InitConfig) -> Result<Command> {
    let user = resolve_run_as_user()?;
    build_command_with_user(config, &user)
}

fn build_command_with_user(config: InitConfig, user: &RunAsUser) -> Result<Command> {
    let path_env = config.env.get("PATH").cloned();
    let mut cmd = match config.entrypoint.split_first() {
        Some((prog, args)) => {
            let mut c = Command::new(prog);
            c.args(args);
            c.args(&config.cmd);
            c
        },
        None => match config.cmd.split_first() {
            Some((prog, args)) => {
                let mut c = Command::new(prog);
                c.args(args);
                c
            },
            None => return Err(anyhow!("No entrypoint or cmd provided in config")),
        },
    };

    // Set environment variables
    for (key, val) in config.env {
        cmd.env(key, val);
    }

    cmd.env("PATH", effective_path(path_env.as_deref()));

    cmd.env("HOME", &user.dir);
    cmd.env("USER", &user.name);
    cmd.env("LOGNAME", &user.name);

    // Set working directory
    let workdir = Path::new(&config.workdir);
    if !workdir.exists() {
        fs::create_dir_all(workdir).with_context(|| {
            format!("Failed to create working directory: {}", workdir.display())
        })?;
    }
    cmd.current_dir(workdir);
    cmd.uid(user.uid);
    cmd.gid(user.gid);

    Ok(cmd)
}

fn effective_path(existing: Option<&str>) -> String {
    let mut parts = vec![
        "/app/node_modules/.bin".to_string(),
        "/mise/shims".to_string(),
        "/usr/local/bin".to_string(),
    ];

    if let Some(existing) = existing {
        for part in existing.split(':') {
            if !part.is_empty() && !parts.iter().any(|candidate| candidate == part) {
                parts.push(part.to_string());
            }
        }
    }

    if !parts.iter().any(|part| part == "/usr/local/sbin") {
        parts.push("/usr/local/sbin".to_string());
    }
    if !parts.iter().any(|part| part == "/usr/sbin") {
        parts.push("/usr/sbin".to_string());
    }
    if !parts.iter().any(|part| part == "/usr/bin") {
        parts.push("/usr/bin".to_string());
    }
    if !parts.iter().any(|part| part == "/sbin") {
        parts.push("/sbin".to_string());
    }
    if !parts.iter().any(|part| part == "/bin") {
        parts.push("/bin".to_string());
    }

    parts.join(":")
}

fn spawn_application(config: InitConfig) -> Result<tokio::process::Child> {
    let cmd = build_command(config)?;
    let mut cmd: tokio::process::Command = cmd.into();
    cmd.spawn()
        .with_context(|| "Failed to spawn application process")
}

#[derive(Debug, Clone)]
struct RunAsUser {
    uid: u32,
    gid: u32,
    dir: String,
    name: String,
}

fn resolve_run_as_user() -> Result<RunAsUser> {
    let user = User::from_name("mikrom")
        .context("Failed to resolve mikrom user from passwd database")?
        .ok_or_else(|| anyhow!("User mikrom not found in passwd database"))?;

    Ok(RunAsUser {
        uid: user.uid.as_raw(),
        gid: user.gid.as_raw(),
        dir: user.dir.to_string_lossy().to_string(),
        name: user.name,
    })
}

async fn wait_for_port_ready(port: u16, child: &mut tokio::process::Child) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);

    loop {
        if let Some(status) = child
            .try_wait()
            .context("Failed to poll application process")?
        {
            return Err(anyhow!(
                "Application exited before becoming ready: {status}"
            ));
        }

        let attempts = [
            TcpStream::connect(("127.0.0.1", port)),
            TcpStream::connect(("::1", port)),
        ];

        for attempt in attempts {
            if attempt.await.is_ok() {
                println!("[mikrom-init] Application is accepting connections on port {port}");
                return Ok(());
            }
        }

        if Instant::now() >= deadline {
            return Err(anyhow!(
                "Timed out waiting for application to accept connections on port {port}"
            ));
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn start_background_services() -> Result<()> {
    let sshd_path = "/usr/sbin/sshd";
    if Path::new(sshd_path).exists() {
        println!("[mikrom-init] Starting SSH daemon...");
        let _ = fs::create_dir_all("/run/sshd");

        // Start sshd in background (don't wait for it)
        if let Err(e) = Command::new(sshd_path).spawn() {
            eprintln!("[mikrom-init] Warning: Failed to spawn sshd: {e}");
        }
    }

    Ok(())
}

fn fallback_to_shell() -> ! {
    if Path::new(FALLBACK_SHELL).exists() {
        println!("[mikrom-init] Falling back to {FALLBACK_SHELL}...");
        let _ = Command::new(FALLBACK_SHELL).exec();
    }

    eprintln!("[mikrom-init] CRITICAL: All execution attempts failed. Halting.");
    halt_pid1()
}

fn halt_pid1() -> ! {
    loop {
        std::thread::park();
    }
}

fn setup_volume_mounts(config: &InitConfig) -> Result<()> {
    if config.volumes.is_empty() {
        return Ok(());
    }

    println!("[mikrom-init] Setting up volume mounts...");

    for vol in &config.volumes {
        // Try discovery by serial first
        let device = match find_device_by_serial(&vol.drive_id)? {
            Some(dev) => dev,
            None => {
                // Fallback: Firecracker attaches drives in order.
                // rootfs is /dev/vda (index 0).
                // First extra volume is /dev/vdb (index 1), etc.
                if let Some(idx) = vol.index {
                    let letter = (b'a' + (idx as u8)) as char;
                    let dev = format!("/dev/vd{}", letter);
                    println!(
                        "[mikrom-init] Serial discovery failed for {}, mapped by index {} -> {}",
                        vol.drive_id, idx, dev
                    );
                    dev
                } else {
                    eprintln!(
                        "[mikrom-init] Warning: Device not found for volume {} and no index provided",
                        vol.drive_id
                    );
                    continue;
                }
            },
        };

        println!(
            "[mikrom-init] Mounting {} to {}...",
            device, vol.mount_point
        );

        // Ensure device node exists in /dev (devtmpfs might be slow)
        if !std::path::Path::new(&device).exists() {
            eprintln!(
                "[mikrom-init] Warning: Device node {} does not exist in /dev, wait-and-retry...",
                device
            );
            // Brief wait
            std::thread::sleep(std::time::Duration::from_millis(500));
        }

        // Ensure mount point exists
        if let Err(e) = fs::create_dir_all(&vol.mount_point) {
            eprintln!(
                "[mikrom-init] Warning: Failed to create mount point {}: {}",
                vol.mount_point, e
            );
            continue;
        }

        // Mount the device
        if let Err(e) = mount_fs(&device, &vol.mount_point, "ext4", MsFlags::empty()) {
            eprintln!(
                "[mikrom-init] Warning: Failed to mount {} to {}: {}",
                device, vol.mount_point, e
            );
        }
    }

    Ok(())
}

fn find_device_by_serial(drive_id: &str) -> Result<Option<String>> {
    // Virtio-blk serial IDs are often truncated to 20 characters in the guest kernel.
    let target_serial = if drive_id.len() > 20 {
        &drive_id[..20]
    } else {
        drive_id
    };

    println!(
        "[mikrom-init] Looking for device with serial: {} (original: {})",
        target_serial, drive_id
    );

    // Retry for up to 5 seconds as devices might take a moment to appear
    for attempt in 1..=10 {
        if attempt > 1 {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }

        let block_dir = match fs::read_dir("/sys/block") {
            Ok(dir) => dir,
            Err(e) => {
                eprintln!("[mikrom-init] Warning: Failed to read /sys/block: {}", e);
                continue;
            },
        };

        for entry in block_dir {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("vd") {
                continue;
            }

            let serial_path = format!("/sys/block/{}/serial", name);
            if let Ok(serial) = fs::read_to_string(&serial_path) {
                let found_serial = serial.trim();
                if found_serial == target_serial || drive_id.starts_with(found_serial) {
                    return Ok(Some(format!("/dev/{}", name)));
                }
            }
        }
    }

    Ok(None)
}

fn mount_fs(source: &str, target: &str, fstype: &str, flags: MsFlags) -> Result<()> {
    if !Path::new(target).exists() {
        fs::create_dir_all(target)
            .with_context(|| format!("Failed to create mount point: {target}"))?;
    }

    mount(Some(source), target, Some(fstype), flags, None::<&str>)
        .map_err(|e| anyhow!("Failed to mount {source} on {target} ({fstype}): {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_deserialization() {
        let json = r#"{
            "env": {"FOO": "bar"},
            "entrypoint": ["/bin/whoami"],
            "cmd": ["--help"]
        }"#;
        let config: InitConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.env.get("FOO").unwrap(), "bar");
        assert_eq!(config.entrypoint[0], "/bin/whoami");
        assert_eq!(config.cmd[0], "--help");
        assert_eq!(config.workdir, "/app"); // default
    }

    #[test]
    fn test_config_minimal() {
        let json = r#"{
            "entrypoint": ["ls"]
        }"#;
        let config: InitConfig = serde_json::from_str(json).unwrap();
        assert!(config.env.is_empty());
        assert_eq!(config.entrypoint[0], "ls");
        assert!(config.cmd.is_empty());
    }

    #[test]
    fn test_build_command_entrypoint() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = InitConfig {
            env: HashMap::new(),
            workdir: temp_dir
                .path()
                .join("test-app")
                .to_string_lossy()
                .to_string(),
            entrypoint: vec!["/bin/sh".to_string(), "-c".to_string()],
            cmd: vec!["echo hello".to_string()],
            volumes: vec![],
        };
        let user = RunAsUser {
            uid: 1000,
            gid: 1000,
            dir: "/home/mikrom".to_string(),
            name: "mikrom".to_string(),
        };
        let cmd = build_command_with_user(config, &user).unwrap();

        assert_eq!(cmd.get_program(), "/bin/sh");
        let args: Vec<_> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        assert_eq!(args, vec!["-c", "echo hello"]);
        assert_eq!(
            cmd.get_current_dir().unwrap(),
            temp_dir.path().join("test-app")
        );

        let envs: HashMap<_, _> = cmd
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().to_string(),
                    value.map(|v| v.to_string_lossy().to_string()),
                )
            })
            .collect();

        assert_eq!(
            envs.get("PATH").and_then(|value| value.clone()).unwrap(),
            effective_path(None)
        );
        assert_eq!(
            envs.get("HOME").and_then(|value| value.clone()).unwrap(),
            "/home/mikrom"
        );
        assert_eq!(
            envs.get("USER").and_then(|value| value.clone()).unwrap(),
            "mikrom"
        );
        assert_eq!(
            envs.get("LOGNAME").and_then(|value| value.clone()).unwrap(),
            "mikrom"
        );
    }

    #[test]
    fn test_build_command_uses_cmd_when_entrypoint_missing() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = InitConfig {
            env: HashMap::from([(
                "PATH".to_string(),
                "/usr/local/bin:/usr/bin:/bin".to_string(),
            )]),
            workdir: temp_dir.path().join("app").to_string_lossy().to_string(),
            entrypoint: vec![],
            cmd: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "echo hello".to_string(),
            ],
            volumes: vec![],
        };
        let user = RunAsUser {
            uid: 1000,
            gid: 1000,
            dir: "/home/mikrom".to_string(),
            name: "mikrom".to_string(),
        };

        let cmd = build_command_with_user(config, &user).unwrap();
        assert_eq!(cmd.get_program(), "/bin/sh");
        let args: Vec<_> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        assert_eq!(args, vec!["-c", "echo hello"]);
    }

    #[test]
    fn test_effective_path_deduplicates_existing_entries() {
        let path = effective_path(Some(
            "/mise/shims:/usr/local/bin:/usr/local/bin:/bin:/custom/bin",
        ));
        let parts: Vec<_> = path.split(':').collect();
        assert_eq!(
            parts
                .iter()
                .filter(|part| **part == "/usr/local/bin")
                .count(),
            1
        );
        assert_eq!(
            parts.iter().filter(|part| **part == "/mise/shims").count(),
            1
        );
        assert!(parts.contains(&"/custom/bin"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_wait_for_port_ready_detects_listener() {
        let listener = match tokio::net::TcpListener::bind(("127.0.0.1", 0)).await {
            Ok(listener) => listener,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(e) => panic!("failed to bind test listener: {e}"),
        };
        let port = listener.local_addr().unwrap().port();
        let accept_task = tokio::spawn(async move {
            let _ = listener.accept().await;
        });

        let mut child = tokio::process::Command::new("sleep")
            .arg("1")
            .spawn()
            .unwrap();

        wait_for_port_ready(port, &mut child).await.unwrap();
        let _ = child.kill().await;
        let _ = accept_task.await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_start_background_services_missing_sshd() {
        // Should not panic or return error if sshd is missing
        let result = start_background_services().await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_effective_path_prepends_mise_and_app_bins() {
        let path = effective_path(Some("/usr/local/sbin:/usr/local/bin:/usr/bin:/bin"));
        assert!(path.starts_with("/app/node_modules/.bin:/mise/shims:/usr/local/bin"));
        assert!(path.contains("/usr/local/sbin"));
        assert!(path.contains("/usr/bin"));
        assert!(path.contains("/bin"));
    }

    #[test]
    fn test_effective_path_defaults_when_missing() {
        let path = effective_path(None);
        assert!(path.starts_with("/app/node_modules/.bin:/mise/shims:/usr/local/bin"));
    }

    #[test]
    fn test_build_hosts_contents_matches_expected_format() {
        let hosts = build_hosts_contents("localhost");
        assert!(hosts.contains("127.0.0.1 localhost"));
        assert!(hosts.contains("::1 localhost ip6-localhost ip6-loopback"));
        assert!(hosts.contains("127.0.1.1 localhost"));
    }
}
