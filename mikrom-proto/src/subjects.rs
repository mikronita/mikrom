//! NATS subjects used across the mikrom workspace.

/// Typed representation of the shared NATS subjects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SharedSubject {
    RouterConfigUpdated,
    RouterTlsCertUpdated,
    RouterAcmeChallengeUpdated,
    RouterTrafficEvent,
    SchedulerJobUpdates,
    SchedulerWorkerHeartbeat,
    SchedulerRouterHeartbeat,
    SchedulerVmFailed,
    SchedulerListApps,
    SchedulerListWorkers,
    SchedulerDeploy,
    SchedulerPauseApp,
    SchedulerResumeApp,
    SchedulerDeleteApp,
    SchedulerScaleApp,
    SchedulerUpdateAppScalingConfig,
    SchedulerCancelApp,
    SchedulerGetJob,
    SchedulerDeployDatabase,
    SchedulerListDatabases,
    SchedulerGetDatabaseStatus,
    SchedulerDeleteDatabase,
    BuilderBuild,
    BuilderGetStatus,
}

impl SharedSubject {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RouterConfigUpdated => "mikrom.router.config_updated",
            Self::RouterTlsCertUpdated => "mikrom.router.tls_cert_updated",
            Self::RouterAcmeChallengeUpdated => "mikrom.router.acme_challenge_updated",
            Self::RouterTrafficEvent => "mikrom.router.traffic",
            Self::SchedulerJobUpdates => "mikrom.scheduler.job_updates",
            Self::SchedulerWorkerHeartbeat => "mikrom.scheduler.worker.heartbeat",
            Self::SchedulerRouterHeartbeat => "mikrom.scheduler.router.heartbeat",
            Self::SchedulerVmFailed => "mikrom.scheduler.vm_failed",
            Self::SchedulerListApps => "mikrom.scheduler.list_apps",
            Self::SchedulerListWorkers => "mikrom.scheduler.list_workers",
            Self::SchedulerDeploy => "mikrom.scheduler.deploy",
            Self::SchedulerPauseApp => "mikrom.scheduler.pause_app",
            Self::SchedulerResumeApp => "mikrom.scheduler.resume_app",
            Self::SchedulerDeleteApp => "mikrom.scheduler.delete_app",
            Self::SchedulerScaleApp => "mikrom.scheduler.scale_app",
            Self::SchedulerUpdateAppScalingConfig => "mikrom.scheduler.update_app_scaling_config",
            Self::SchedulerCancelApp => "mikrom.scheduler.cancel_app",
            Self::SchedulerGetJob => "mikrom.scheduler.get_job",
            Self::SchedulerDeployDatabase => "mikrom.scheduler.database.deploy",
            Self::SchedulerListDatabases => "mikrom.scheduler.database.list",
            Self::SchedulerGetDatabaseStatus => "mikrom.scheduler.database.status",
            Self::SchedulerDeleteDatabase => "mikrom.scheduler.database.delete",
            Self::BuilderBuild => "mikrom.builder.build",
            Self::BuilderGetStatus => "mikrom.builder.get_status",
        }
    }
}

impl From<SharedSubject> for &'static str {
    fn from(subject: SharedSubject) -> Self {
        subject.as_str()
    }
}

impl From<SharedSubject> for String {
    fn from(subject: SharedSubject) -> Self {
        subject.as_str().to_string()
    }
}

impl std::fmt::Display for SharedSubject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// Backwards-compatible aliases for existing callers.
pub const ROUTER_CONFIG_UPDATED: &str = SharedSubject::RouterConfigUpdated.as_str();
pub const ROUTER_TLS_CERT_UPDATED: &str = SharedSubject::RouterTlsCertUpdated.as_str();
pub const ROUTER_ACME_CHALLENGE_UPDATED: &str = SharedSubject::RouterAcmeChallengeUpdated.as_str();
pub const ROUTER_TRAFFIC_EVENT: &str = SharedSubject::RouterTrafficEvent.as_str();
pub const SCHEDULER_JOB_UPDATES: &str = SharedSubject::SchedulerJobUpdates.as_str();
pub const SCHEDULER_WORKER_HEARTBEAT: &str = SharedSubject::SchedulerWorkerHeartbeat.as_str();
pub const SCHEDULER_ROUTER_HEARTBEAT: &str = SharedSubject::SchedulerRouterHeartbeat.as_str();
pub const SCHEDULER_VM_FAILED: &str = SharedSubject::SchedulerVmFailed.as_str();
pub const SCHEDULER_LIST_APPS: &str = SharedSubject::SchedulerListApps.as_str();
pub const SCHEDULER_LIST_WORKERS: &str = SharedSubject::SchedulerListWorkers.as_str();
pub const SCHEDULER_DEPLOY: &str = SharedSubject::SchedulerDeploy.as_str();
pub const SCHEDULER_PAUSE_APP: &str = SharedSubject::SchedulerPauseApp.as_str();
pub const SCHEDULER_RESUME_APP: &str = SharedSubject::SchedulerResumeApp.as_str();
pub const SCHEDULER_DELETE_APP: &str = SharedSubject::SchedulerDeleteApp.as_str();
pub const SCHEDULER_SCALE_APP: &str = SharedSubject::SchedulerScaleApp.as_str();
pub const SCHEDULER_UPDATE_APP_SCALING_CONFIG: &str =
    SharedSubject::SchedulerUpdateAppScalingConfig.as_str();
pub const SCHEDULER_CANCEL_APP: &str = SharedSubject::SchedulerCancelApp.as_str();
pub const SCHEDULER_GET_JOB: &str = SharedSubject::SchedulerGetJob.as_str();
pub const SCHEDULER_DEPLOY_DATABASE: &str = SharedSubject::SchedulerDeployDatabase.as_str();
pub const SCHEDULER_LIST_DATABASES: &str = SharedSubject::SchedulerListDatabases.as_str();
pub const SCHEDULER_GET_DATABASE_STATUS: &str = SharedSubject::SchedulerGetDatabaseStatus.as_str();
pub const SCHEDULER_DELETE_DATABASE: &str = SharedSubject::SchedulerDeleteDatabase.as_str();
pub const BUILDER_BUILD: &str = SharedSubject::BuilderBuild.as_str();
pub const BUILDER_GET_STATUS: &str = SharedSubject::BuilderGetStatus.as_str();

/// Subject prefix for VM logs.
pub const LOGS_PREFIX: &str = "mikrom.logs";

/// Returns the subject for a specific VM's logs.
pub fn vm_logs(vm_id: &str) -> String {
    format!("{LOGS_PREFIX}.{vm_id}")
}

/// Returns the subject for a specific builder's status.
pub fn builder_status(build_id: &str) -> String {
    format!("mikrom.builder.{build_id}.status")
}
