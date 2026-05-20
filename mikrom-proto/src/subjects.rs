//! NATS subjects used across the mikrom workspace.

// ── Router Subjects ──────────────────────────────────────────────────────────

/// Router configuration update subject.
pub const ROUTER_CONFIG_UPDATED: &str = "mikrom.router.config_updated";

/// Router TLS certificate update subject.
pub const ROUTER_TLS_CERT_UPDATED: &str = "mikrom.router.tls_cert_updated";

/// Router ACME challenge update subject.
pub const ROUTER_ACME_CHALLENGE_UPDATED: &str = "mikrom.router.acme_challenge_updated";

/// Router traffic event subject.
pub const ROUTER_TRAFFIC_EVENT: &str = "mikrom.router.traffic";

// ── Scheduler Subjects ───────────────────────────────────────────────────────

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

/// Scheduler scale app subject (request/reply).
pub const SCHEDULER_SCALE_APP: &str = "mikrom.scheduler.scale_app";

/// Scheduler update app scaling config subject (request/reply).
pub const SCHEDULER_UPDATE_APP_SCALING_CONFIG: &str = "mikrom.scheduler.update_app_scaling_config";

/// Scheduler cancel app subject (request/reply).
pub const SCHEDULER_CANCEL_APP: &str = "mikrom.scheduler.cancel_app";

/// Scheduler get job subject (request/reply).
pub const SCHEDULER_GET_JOB: &str = "mikrom.scheduler.get_job";

// ── Builder Subjects ─────────────────────────────────────────────────────────

/// Builder build subject (request/reply).
pub const BUILDER_BUILD: &str = "mikrom.builder.build";

/// Builder get status subject (request/reply).
pub const BUILDER_GET_STATUS: &str = "mikrom.builder.get_status";

/// Subject prefix for VM logs.
pub const LOGS_PREFIX: &str = "mikrom.logs";

// ── Helper Functions ─────────────────────────────────────────────────────────

/// Returns the subject for a specific VM's logs.
pub fn vm_logs(vm_id: &str) -> String {
    format!("{LOGS_PREFIX}.{vm_id}")
}

/// Returns the subject for a specific builder's status.
pub fn builder_status(build_id: &str) -> String {
    format!("mikrom.builder.{build_id}.status")
}
