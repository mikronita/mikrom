use crate::hypervisor::HypervisorError;
use std::path::PathBuf;
use tokio::process::Command;

pub struct CloudHypervisorProcess {
    pub vm_id: String,
    pub socket_path: PathBuf,
    pub pid: u32,
    pub child: tokio::process::Child,
    pub sidecars: Vec<tokio::process::Child>,
    pub tap_ifindex: Option<u32>,
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
            child,
            sidecars: Vec::new(),
            tap_ifindex,
        })
    }

    pub async fn kill(&mut self) -> Result<(), HypervisorError> {
        for mut sidecar in self.sidecars.drain(..) {
            let _ = sidecar.kill().await;
        }
        self.child
            .kill()
            .await
            .map_err(|e| HypervisorError::ProcessError(format!("Failed to kill CH process: {e}")))
    }
}
