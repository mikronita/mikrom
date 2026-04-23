use nix::mount::{MsFlags, mount};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

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

fn main() -> anyhow::Result<()> {
    println!("[mikrom-init] Initializing microVM environment...");

    // 1. Mount essential filesystems
    mount_fs("proc", "/proc", "proc", MsFlags::empty())?;
    mount_fs("sysfs", "/sys", "sysfs", MsFlags::empty())?;

    // devtmpfs might fail if not supported by kernel, so we try and ignore error
    let _ = mount_fs("devtmpfs", "/dev", "devtmpfs", MsFlags::empty());

    // Create mount points if they don't exist
    let dirs = ["/run", "/tmp", "/dev/pts", "/dev/shm"];
    for dir in &dirs {
        let _ = fs::create_dir_all(dir);
    }

    mount_fs("tmpfs", "/run", "tmpfs", MsFlags::empty())?;
    mount_fs("tmpfs", "/tmp", "tmpfs", MsFlags::empty())?;
    mount_fs("tmpfs", "/dev/shm", "tmpfs", MsFlags::empty())?;
    let _ = mount_fs("devpts", "/dev/pts", "devpts", MsFlags::empty());

    // 2. Set hostname
    let _ = nix::unistd::sethostname("localhost");

    // 3. Bring up loopback interface (optional but good practice)
    let _ = Command::new("ip")
        .args(["link", "set", "lo", "up"])
        .status();

    // 4. Load configuration
    let config_path = "/etc/mikrom/init.json";
    if !Path::new(config_path).exists() {
        println!(
            "[mikrom-init] Error: Configuration file not found at {}",
            config_path
        );
        // Fallback to a shell if it exists, otherwise panic
        if Path::new("/bin/sh").exists() {
            println!("[mikrom-init] Falling back to /bin/sh");
            let _ = Command::new("/bin/sh").exec();
        }
        panic!("Initialization failed: No config and no fallback shell");
    }

    let config_str = fs::read_to_string(config_path)?;
    let config: InitConfig = serde_json::from_str(&config_str)?;

    // 5. Prepare execution
    println!(
        "[mikrom-init] Starting application: {:?}",
        config.entrypoint
    );

    let mut cmd = if !config.entrypoint.is_empty() {
        let mut c = Command::new(&config.entrypoint[0]);
        if config.entrypoint.len() > 1 {
            c.args(&config.entrypoint[1..]);
        }
        c.args(&config.cmd);
        c
    } else if !config.cmd.is_empty() {
        let mut c = Command::new(&config.cmd[0]);
        if config.cmd.len() > 1 {
            c.args(&config.cmd[1..]);
        }
        c
    } else {
        panic!("No entrypoint or cmd provided in config");
    };

    // Set environment variables
    for (key, val) in config.env {
        cmd.env(key, val);
    }

    // Set working directory
    if Path::new(&config.workdir).exists() {
        cmd.current_dir(&config.workdir);
    } else {
        let _ = fs::create_dir_all(&config.workdir);
        cmd.current_dir(&config.workdir);
    }

    // 6. EXECUTE (Replacing mikrom-init as PID 1)
    let err = cmd.exec();

    // If exec() returns, it failed
    println!("[mikrom-init] Failed to execute application: {}", err);

    Ok(())
}

fn mount_fs(source: &str, target: &str, fstype: &str, flags: MsFlags) -> anyhow::Result<()> {
    if !Path::new(target).exists() {
        fs::create_dir_all(target)?;
    }

    mount(Some(source), target, Some(fstype), flags, None::<&str>)
        .map_err(|e| anyhow::anyhow!("Failed to mount {}: {}", target, e))?;

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
}
