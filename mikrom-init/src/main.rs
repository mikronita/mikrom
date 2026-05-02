use anyhow::{Context, Result, anyhow};
use nix::mount::{MsFlags, mount};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

const CONFIG_PATH: &str = "/etc/mikrom/init.json";
const FALLBACK_SHELL: &str = "/bin/sh";

#[derive(Debug, Serialize, Deserialize)]
struct InitConfig {
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default = "default_workdir")]
    workdir: String,
    entrypoint: Vec<String>,
    #[serde(default)]
    cmd: Vec<String>,
}

fn default_workdir() -> String {
    "/app".to_string()
}

fn main() -> Result<()> {
    println!("[mikrom-init] Initializing microVM environment...");

    if let Err(e) = setup_mounts() {
        eprintln!("[mikrom-init] Warning: Mount setup encountered errors: {e}");
    }

    if let Err(e) = setup_system() {
        eprintln!("[mikrom-init] Warning: System setup encountered errors: {e}");
    }

    let config = match load_config(CONFIG_PATH) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("[mikrom-init] Error loading configuration: {e}");
            fallback_to_shell();
        },
    };

    println!(
        "[mikrom-init] Starting application: {:?}",
        config.entrypoint
    );

    let mut cmd = build_command(config)?;

    // EXECUTE (Replacing mikrom-init as PID 1)
    let err = cmd.exec();

    // If exec() returns, it failed
    eprintln!("[mikrom-init] Failed to execute application: {err}");

    fallback_to_shell();
}

fn setup_mounts() -> Result<()> {
    // 1. Mount essential filesystems
    mount_fs("proc", "/proc", "proc", MsFlags::empty())?;
    mount_fs("sysfs", "/sys", "sysfs", MsFlags::empty())?;

    // devtmpfs might fail if not supported by kernel
    if let Err(e) = mount_fs("devtmpfs", "/dev", "devtmpfs", MsFlags::empty()) {
        eprintln!("[mikrom-init] Warning: Failed to mount /dev: {e}");
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

fn setup_system() -> Result<()> {
    // Set hostname
    let _ = nix::unistd::sethostname("localhost");

    // Bring up loopback interface
    let status = Command::new("ip")
        .args(["link", "set", "lo", "up"])
        .status();

    if let Err(e) = status {
        eprintln!("[mikrom-init] Warning: Failed to bring up loopback: {e}");
    }

    Ok(())
}

fn load_config(path: &str) -> Result<InitConfig> {
    let config_str = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file at {path}"))?;

    serde_json::from_str(&config_str).with_context(|| "Failed to parse configuration JSON")
}

fn build_command(config: InitConfig) -> Result<Command> {
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

    // Set working directory
    let workdir = Path::new(&config.workdir);
    if !workdir.exists() {
        fs::create_dir_all(workdir).with_context(|| {
            format!("Failed to create working directory: {}", workdir.display())
        })?;
    }
    cmd.current_dir(workdir);

    Ok(cmd)
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
        let config = InitConfig {
            env: HashMap::new(),
            workdir: "./target/test-app".to_string(),
            entrypoint: vec!["/bin/sh".to_string(), "-c".to_string()],
            cmd: vec!["echo hello".to_string()],
        };
        let _cmd = build_command(config).unwrap();
        let _ = fs::remove_dir_all("./target/test-app");
    }
}
