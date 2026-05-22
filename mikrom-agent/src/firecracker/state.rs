use crate::firecracker::process::VmProcess;
use crate::hypervisor::{VmInfo, VmStatus};
use anyhow::Context;
use mikrom_proto::id::VmId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PersistedVmRuntime {
    pub(crate) vm: VmInfo,
    pub(crate) pid: Option<u32>,
    pub(crate) socket_path: String,
    pub(crate) metrics_path: Option<String>,
    #[serde(default)]
    pub(crate) stdout_log_path: String,
    #[serde(default)]
    pub(crate) stderr_log_path: String,
    #[serde(default)]
    pub(crate) stdout_log_offset: u64,
    #[serde(default)]
    pub(crate) stderr_log_offset: u64,
    pub(crate) tap_name: Option<String>,
    pub(crate) tap_ifindex: Option<u32>,
    pub(crate) chroot_dir: Option<String>,
    pub(crate) app_started: bool,
    pub(crate) app_started_at_ms: u64,
    #[serde(default)]
    pub(crate) vfs_pids: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum PersistedVmRecord {
    Legacy(VmInfo),
    Current(PersistedVmRuntime),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct PersistedAgentState {
    pub(crate) vms: Vec<PersistedVmRecord>,
}

impl PersistedVmRuntime {
    pub(crate) fn from_runtime(vm: &VmInfo, proc: Option<&VmProcess>) -> Self {
        let pid = proc.and_then(crate::firecracker::FirecrackerManager::process_pid);
        let socket_path = proc.map(|p| p.socket_path.clone()).unwrap_or_default();
        let metrics_path = proc.and_then(|p| p.metrics_path.clone());
        let stdout_log_path = proc.map(|p| p.stdout_log_path.clone()).unwrap_or_default();
        let stderr_log_path = proc.map(|p| p.stderr_log_path.clone()).unwrap_or_default();
        let stdout_log_offset = proc
            .map(|p| p.stdout_log_offset.load(Ordering::SeqCst))
            .unwrap_or_default();
        let stderr_log_offset = proc
            .map(|p| p.stderr_log_offset.load(Ordering::SeqCst))
            .unwrap_or_default();
        let tap_name = proc.and_then(|p| p.tap_name.clone());
        let tap_ifindex = proc.and_then(|p| p.tap_ifindex);
        let chroot_dir = proc.and_then(|p| p.chroot_dir.clone());
        let app_started = proc
            .map(|p| p.app_started.load(Ordering::SeqCst))
            .unwrap_or(false);
        let app_started_at_ms = proc
            .map(|p| p.app_started_at_ms.load(Ordering::SeqCst))
            .unwrap_or(0);
        let vfs_pids = proc.map(|p| p.vfs_pids.clone()).unwrap_or_default();

        Self {
            vm: vm.clone(),
            pid,
            socket_path,
            metrics_path,
            stdout_log_path,
            stderr_log_path,
            stdout_log_offset,
            stderr_log_offset,
            tap_name,
            tap_ifindex,
            chroot_dir,
            app_started,
            app_started_at_ms,
            vfs_pids,
        }
    }
}

impl crate::firecracker::FirecrackerManager {
    pub(crate) fn process_pid(proc: &VmProcess) -> Option<u32> {
        proc.pid
            .or_else(|| proc.child.as_ref().and_then(|child| child.id()))
    }

    pub(crate) fn runtime_state_path(&self) -> PathBuf {
        std::path::Path::new(&self.fc_config.data_dir).join("agent-state.json")
    }

    /// Recover a `String` field from persisted runtime, falling back to a
    /// default if the runtime is missing or the field is empty.
    fn recovered_string<F>(
        runtime: Option<&PersistedVmRuntime>,
        extract: impl Fn(&PersistedVmRuntime) -> &str,
        default: F,
    ) -> String
    where
        F: FnOnce() -> String,
    {
        runtime
            .map(extract)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(default)
    }

    /// Recover an `Option<String>` field from persisted runtime, falling back
    /// to a default if the runtime is missing or the field is None/empty.
    fn recovered_opt_string<F>(
        runtime: Option<&PersistedVmRuntime>,
        extract: impl Fn(&PersistedVmRuntime) -> Option<&String>,
        default: F,
    ) -> Option<String>
    where
        F: FnOnce() -> Option<String>,
    {
        runtime.and_then(extract).cloned().or_else(default)
    }

    pub(crate) fn recovered_socket_path(
        &self,
        vm_id: &VmId,
        runtime: Option<&PersistedVmRuntime>,
    ) -> String {
        Self::recovered_string(
            runtime,
            |r| &r.socket_path,
            || {
                if self.fc_config.use_jailer {
                    self.get_chroot_dir(vm_id)
                        .join("root/run/firecracker.socket")
                        .to_string_lossy()
                        .to_string()
                } else {
                    crate::firecracker::paths::VmPaths::new(
                        &self.fc_config.data_dir,
                        &self.agent_id,
                        *vm_id,
                    )
                    .socket_path()
                    .to_string_lossy()
                    .to_string()
                }
            },
        )
    }

    pub(crate) fn recovered_metrics_path(
        &self,
        vm_id: &VmId,
        runtime: Option<&PersistedVmRuntime>,
    ) -> Option<String> {
        Self::recovered_opt_string(
            runtime,
            |r| r.metrics_path.as_ref(),
            || {
                if self.fc_config.use_jailer {
                    Some(
                        self.get_chroot_dir(vm_id)
                            .join("root/metrics.json")
                            .to_string_lossy()
                            .to_string(),
                    )
                } else {
                    Some(
                        crate::firecracker::paths::VmPaths::new(
                            &self.fc_config.data_dir,
                            &self.agent_id,
                            *vm_id,
                        )
                        .metrics_path()
                        .to_string_lossy()
                        .to_string(),
                    )
                }
            },
        )
    }

    pub(crate) fn recovered_stdout_log_path(
        &self,
        vm_id: &VmId,
        runtime: Option<&PersistedVmRuntime>,
    ) -> String {
        Self::recovered_string(
            runtime,
            |r| &r.stdout_log_path,
            || {
                crate::firecracker::paths::VmPaths::new(
                    &self.fc_config.data_dir,
                    &self.agent_id,
                    *vm_id,
                )
                .stdout_log_path()
                .to_string_lossy()
                .to_string()
            },
        )
    }

    pub(crate) fn recovered_stderr_log_path(
        &self,
        vm_id: &VmId,
        runtime: Option<&PersistedVmRuntime>,
    ) -> String {
        Self::recovered_string(
            runtime,
            |r| &r.stderr_log_path,
            || {
                crate::firecracker::paths::VmPaths::new(
                    &self.fc_config.data_dir,
                    &self.agent_id,
                    *vm_id,
                )
                .stderr_log_path()
                .to_string_lossy()
                .to_string()
            },
        )
    }

    pub(crate) async fn load_runtime_state(&self) -> anyhow::Result<()> {
        let state_path = self.runtime_state_path();
        let Ok(raw) = tokio::fs::read_to_string(&state_path).await else {
            return Ok(());
        };

        let state: PersistedAgentState = serde_json::from_str(&raw).with_context(|| {
            format!(
                "Failed to parse runtime state from {}",
                state_path.display()
            )
        })?;

        let mut loaded_vms = HashMap::new();
        let mut loaded_processes = HashMap::new();
        let mut updated_state = false;

        for record in state.vms {
            let (mut vm, runtime) = match record {
                PersistedVmRecord::Legacy(vm) => (vm, None),
                PersistedVmRecord::Current(runtime) => {
                    let vm = runtime.vm.clone();
                    (vm, Some(runtime))
                },
            };

            let runtime_ref = runtime.as_ref();
            let should_track_process = matches!(
                vm.status,
                VmStatus::Starting | VmStatus::Running | VmStatus::Stopping
            );

            if should_track_process {
                let pid = runtime_ref
                    .and_then(|r| r.pid)
                    .filter(|pid| Self::is_pid_alive(*pid));
                if let Some(pid) = pid {
                    let socket_path = self.recovered_socket_path(&vm.vm_id, runtime_ref);
                    let metrics_path = self.recovered_metrics_path(&vm.vm_id, runtime_ref);
                    let stdout_log_path = self.recovered_stdout_log_path(&vm.vm_id, runtime_ref);
                    let stderr_log_path = self.recovered_stderr_log_path(&vm.vm_id, runtime_ref);
                    let stdout_log_offset = Arc::new(AtomicU64::new(
                        runtime_ref.map(|r| r.stdout_log_offset).unwrap_or(0),
                    ));
                    let stderr_log_offset = Arc::new(AtomicU64::new(
                        runtime_ref.map(|r| r.stderr_log_offset).unwrap_or(0),
                    ));
                    let tap_name = runtime_ref.and_then(|r| r.tap_name.clone());
                    let tap_ifindex = runtime_ref.and_then(|r| r.tap_ifindex);
                    let chroot_dir = runtime_ref.and_then(|r| r.chroot_dir.clone()).or_else(|| {
                        if self.fc_config.use_jailer {
                            Some(self.get_chroot_dir(&vm.vm_id).to_string_lossy().to_string())
                        } else {
                            None
                        }
                    });
                    let app_started = Arc::new(AtomicBool::new(
                        runtime_ref.map(|r| r.app_started).unwrap_or(false),
                    ));
                    let app_started_at_ms = Arc::new(AtomicU64::new(
                        runtime_ref.map(|r| r.app_started_at_ms).unwrap_or(0),
                    ));
                    let log_task = self
                        .spawn_log_task_from_paths(
                            &vm.vm_id,
                            &vm.app_id,
                            stdout_log_path.clone(),
                            stderr_log_path.clone(),
                            stdout_log_offset.clone(),
                            stderr_log_offset.clone(),
                            app_started.clone(),
                            app_started_at_ms.clone(),
                        )
                        .await;

                    loaded_processes.insert(
                        vm.vm_id,
                        VmProcess {
                            vm_id: vm.vm_id,
                            child: None,
                            pid: Some(pid),
                            socket_path,
                            metrics_path,
                            stdout_log_path,
                            stderr_log_path,
                            stdout_log_offset,
                            stderr_log_offset,
                            tap_name,
                            tap_ifindex,
                            log_task: Some(log_task),
                            chroot_dir,
                            app_started,
                            app_started_at_ms,
                            vfs_processes: Vec::new(),
                            vfs_pids: runtime_ref.map(|r| r.vfs_pids.clone()).unwrap_or_default(),
                        },
                    );
                } else {
                    vm.status = match vm.status {
                        VmStatus::Stopping => VmStatus::Stopped,
                        _ => VmStatus::Failed,
                    };
                    vm.error_message = Some(
                        "Recovered Firecracker process was not alive after agent restart"
                            .to_string(),
                    );
                    updated_state = true;
                }
            }

            loaded_vms.insert(vm.vm_id, vm);
        }

        {
            let mut vms = self.vms.write().await;
            *vms = loaded_vms;
        }

        {
            let mut processes = self.processes.lock().await;
            *processes = loaded_processes;
        }

        if updated_state {
            let _ = self.persist_runtime_state().await;
        }

        let loaded_vms_count = self.vms.read().await.len();
        let loaded_processes_count = self.processes.lock().await.len();
        tracing::info!(
            state_path = %state_path.display(),
            loaded_vms = loaded_vms_count,
            loaded_processes = loaded_processes_count,
            "Loaded persisted Firecracker state"
        );

        Ok(())
    }

    pub(crate) async fn persist_runtime_state(&self) -> anyhow::Result<()> {
        let state_path = self.runtime_state_path();
        let tmp_path = state_path.with_extension("json.tmp");
        let vms = self.vms.read().await;
        let processes = self.processes.lock().await;
        let mut records = Vec::with_capacity(vms.len());
        for vm in vms.values() {
            let proc = processes.get(&vm.vm_id);
            records.push(PersistedVmRecord::Current(
                PersistedVmRuntime::from_runtime(vm, proc),
            ));
        }
        drop(processes);
        drop(vms);

        let state = PersistedAgentState { vms: records };
        let payload = serde_json::to_vec_pretty(&state)?;

        tokio::fs::write(&tmp_path, payload)
            .await
            .with_context(|| {
                format!(
                    "Failed to write temporary runtime state {}",
                    tmp_path.display()
                )
            })?;
        tokio::fs::rename(&tmp_path, &state_path)
            .await
            .with_context(|| {
                format!(
                    "Failed to persist runtime state to {}",
                    state_path.display()
                )
            })?;

        Ok(())
    }
}
