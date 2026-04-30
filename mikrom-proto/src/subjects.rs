//! NATS subjects used across the mikrom workspace.

/// Router configuration update subject.
pub const ROUTER_CONFIG_UPDATED: &str = "mikrom.router.config_updated";

/// Scheduler job updates subject.
pub const SCHEDULER_JOB_UPDATES: &str = "mikrom.scheduler.job_updates";

/// Scheduler list apps subject (request/reply).
pub const SCHEDULER_LIST_APPS: &str = "mikrom.scheduler.list_apps";

/// Scheduler list workers subject (request/reply).
pub const SCHEDULER_LIST_WORKERS: &str = "mikrom.scheduler.list_workers";

/// Scheduler deploy subject (request/reply).
pub const SCHEDULER_DEPLOY: &str = "mikrom.scheduler.deploy";

/// Scheduler pause app subject (request/reply).
pub const SCHEDULER_PAUSE_APP: &str = "mikrom.scheduler.pause_app";

/// Scheduler resume app subject (request/reply).
pub const SCHEDULER_RESUME_APP: &str = "mikrom.scheduler.resume_app";

/// Scheduler delete app subject (request/reply).
pub const SCHEDULER_DELETE_APP: &str = "mikrom.scheduler.delete_app";

/// Scheduler cancel app subject (request/reply).
pub const SCHEDULER_CANCEL_APP: &str = "mikrom.scheduler.cancel_app";

/// Scheduler get job subject (request/reply).
pub const SCHEDULER_GET_JOB: &str = "mikrom.scheduler.get_job";

/// Builder build subject (request/reply).
pub const BUILDER_BUILD: &str = "mikrom.builder.build";

/// Builder get status subject (request/reply).
pub const BUILDER_GET_STATUS: &str = "mikrom.builder.get_status";

/// Subject prefix for VM logs.
pub const LOGS_PREFIX: &str = "mikrom.logs";

/// Returns the subject for a specific VM's logs.
pub fn vm_logs(vm_id: &str) -> String {
    format!("{}.{}", LOGS_PREFIX, vm_id)
}

/// Returns the subject for a specific builder's status.
pub fn builder_status(build_id: &str) -> String {
    format!("mikrom.builder.{}.status", build_id)
}
