//! NATS subjects used by the router.

use mikrom_proto::subjects::SharedSubject;

pub const ROUTER_SUBJECT_PREFIX: &str = "mikrom.router.";

#[must_use]
pub fn mesh_updates(router_id: &str) -> String {
    format!("mikrom.scheduler.network.mesh.{router_id}")
}

#[must_use]
pub fn router_metrics(router_id: &str) -> String {
    format!("mikrom.metrics.router.{router_id}")
}

pub const ROUTER_CONFIG_UPDATED: &str = SharedSubject::RouterConfigUpdated.as_str();
pub const ROUTER_TLS_CERT_UPDATED: &str = SharedSubject::RouterTlsCertUpdated.as_str();
pub const ROUTER_ACME_CHALLENGE_UPDATED: &str = SharedSubject::RouterAcmeChallengeUpdated.as_str();
pub const ROUTER_TRAFFIC_EVENT: &str = SharedSubject::RouterTrafficEvent.as_str();
pub const SCHEDULER_ROUTER_HEARTBEAT: &str = SharedSubject::SchedulerRouterHeartbeat.as_str();

#[must_use]
pub const fn control_plane_subjects() -> [&'static str; 3] {
    [
        ROUTER_CONFIG_UPDATED,
        ROUTER_TLS_CERT_UPDATED,
        ROUTER_ACME_CHALLENGE_UPDATED,
    ]
}

#[must_use]
pub const fn control_plane_subject_wildcard() -> &'static str {
    "mikrom.router.>"
}
