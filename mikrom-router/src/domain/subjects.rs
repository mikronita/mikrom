//! NATS subjects used by the router.

use mikrom_proto::subjects::SharedSubject;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Subject(String);

impl Subject {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for Subject {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Subject {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<Subject> for String {
    fn from(value: Subject) -> Self {
        value.0
    }
}

impl AsRef<str> for Subject {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for Subject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub const ROUTER_SUBJECT_PREFIX: &str = "mikrom.router.";

#[must_use]
pub fn mesh_updates(router_id: &str) -> Subject {
    Subject::from(format!("mikrom.scheduler.network.mesh.{router_id}"))
}

#[must_use]
pub fn router_metrics(router_id: &str) -> Subject {
    Subject::from(format!("mikrom.metrics.router.{router_id}"))
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
pub fn control_plane_subject_wildcard() -> Subject {
    Subject::from("mikrom.router.>")
}

#[cfg(test)]
mod tests {
    use super::{
        ROUTER_CONFIG_UPDATED, ROUTER_SUBJECT_PREFIX, control_plane_subject_wildcard, mesh_updates,
        router_metrics,
    };

    #[test]
    fn test_constants_not_empty() {
        assert!(!ROUTER_SUBJECT_PREFIX.is_empty());
        assert!(!ROUTER_CONFIG_UPDATED.is_empty());
    }

    #[test]
    fn test_control_plane_subject_wildcard() {
        assert_eq!(control_plane_subject_wildcard().as_str(), "mikrom.router.>");
    }

    #[test]
    fn test_dynamic_subjects_are_wrapped() {
        assert_eq!(
            mesh_updates("router-1").as_str(),
            "mikrom.scheduler.network.mesh.router-1"
        );
        assert_eq!(
            router_metrics("router-1").as_str(),
            "mikrom.metrics.router.router-1"
        );
    }
}
