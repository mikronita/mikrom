/// Execute an async block with the QMP client for a VM.
///
/// Macro that expands the ~12-line boilerplate (lock processes,
/// lookup VM, check QMP, lock QMP client) so each QMP method stays
/// focused on the actual command.
///
/// Usage:
/// ```ignore
/// with_qmp!(self, vm_id, "snapshot", |qmp| {
///     qmp.human_monitor_command("savevm snap").await?;
///     Ok(())
/// })
/// ```
macro_rules! with_qmp {
    ($self:expr, $vm_id:expr, $context:expr, |$qmp:ident| $body:expr) => {{
        let procs = $self.processes.lock().await;
        let proc = procs
            .get($vm_id)
            .ok_or_else(|| HypervisorError::VmNotFound($vm_id.to_string()))?;

        let qmp_mutex = proc.qmp.as_ref().ok_or_else(|| {
            HypervisorError::ProcessError(format!("QMP not available for {}", $context))
        })?;

        let mut $qmp = qmp_mutex.lock().await;
        $body
    }};
}

pub mod api;
pub mod cleanup;
pub mod config;
pub mod manager;
pub mod qmp;
pub mod startup;
pub mod state;

pub mod balloon;
pub mod console;
pub mod metrics;
pub mod migration;
pub mod snapshots;
pub mod volumes;

pub use config::QemuConfig;
pub use manager::QemuManager;
pub use qmp::{QmpClient, QmpError};
