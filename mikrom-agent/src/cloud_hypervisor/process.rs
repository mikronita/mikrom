use crate::hypervisor::HypervisorError;
use std::path::PathBuf;
use tokio::process::Command;

pub struct CloudHypervisorProcess {
    pub vm_id: String,
    pub socket_path: PathBuf,
    pub pid: u32,
    pub child: Option<tokio::process::Child>,
    pub sidecars: Vec<tokio::process::Child>,
    pub tap_ifindex: Option<u32>,
}

fn is_pid_alive(pid: u32) -> bool {
    let status_path = format!("/proc/{pid}/status");
    match std::fs::read_to_string(status_path) {
        Ok(status) => !status.lines().any(|line| line.starts_with("State:\tZ")),
        Err(_) => false,
    }
}

impl CloudHypervisorProcess {
    pub async fn spawn(
        binary_path: &PathBuf,
        vm_id: String,
        socket_path: PathBuf,
        log_path: PathBuf,
        tap_ifindex: Option<u32>,
    ) -> Result<Self, HypervisorError> {
        let log_file = std::fs::File::create(&log_path).map_err(|e| {
            HypervisorError::ProcessError(format!("Failed to create log file: {e}"))
        })?;

        let mut cmd = Command::new(binary_path);
        cmd.kill_on_drop(true);

        cmd.arg("--api-socket")
            .arg(&socket_path)
            .arg("-v") // Verbose logging
            .stdout(log_file.try_clone().unwrap())
            .stderr(log_file);

        let child = cmd.spawn().map_err(|e| {
            HypervisorError::ProcessError(format!("Failed to spawn Cloud Hypervisor: {e}"))
        })?;

        let pid = child
            .id()
            .ok_or_else(|| HypervisorError::ProcessError("Failed to get CH PID".to_string()))?;

        tracing::info!(vm_id = %vm_id, pid = %pid, "Spawned Cloud Hypervisor process");

        Ok(Self {
            vm_id,
            socket_path,
            pid,
            child: Some(child),
            sidecars: Vec::new(),
            tap_ifindex,
        })
    }

    pub async fn kill(&mut self) -> Result<(), HypervisorError> {
        for mut sidecar in self.sidecars.drain(..) {
            let _ = sidecar.kill().await;
        }
        if let Some(mut child) = self.child.take() {
            child.kill().await.map_err(|e| {
                HypervisorError::ProcessError(format!("Failed to kill CH process: {e}"))
            })?;
        } else {
            // Recovered process: use kill signal at OS level
            let rc = unsafe { libc::kill(self.pid as i32, libc::SIGTERM) };
            if rc != 0 {
                let err = std::io::Error::last_os_error();
                tracing::warn!(vm_id = %self.vm_id, pid = self.pid, error = %err, "Failed to send SIGTERM to recovered Cloud Hypervisor process");
            }
            // Wait for exit
            let mut exited = false;
            for _ in 0..20 {
                if !is_pid_alive(self.pid) {
                    exited = true;
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            if !exited {
                tracing::warn!(vm_id = %self.vm_id, pid = self.pid, "SIGTERM timed out for recovered Cloud Hypervisor process, sending SIGKILL");
                let rc = unsafe { libc::kill(self.pid as i32, libc::SIGKILL) };
                if rc != 0 {
                    let err = std::io::Error::last_os_error();
                    tracing::warn!(vm_id = %self.vm_id, pid = self.pid, error = %err, "Failed to send SIGKILL to recovered Cloud Hypervisor process");
                }
            }
        }
        Ok(())
    }
}
