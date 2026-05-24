use pingora::prelude::*;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::RwLock;

#[derive(Default)]
pub struct RouterHealth {
    bootstrapped: AtomicBool,
    dependencies_ready: AtomicBool,
    control_plane_synced: AtomicBool,
    wireguard_ready: AtomicBool,
    upstream_ca_ready: AtomicBool,
    startup_error: RwLock<Option<String>>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct HealthSnapshot {
    pub live: bool,
    pub ready: bool,
    pub dependencies_ready: bool,
    pub control_plane_synced: bool,
    pub wireguard_ready: bool,
    pub upstream_ca_ready: bool,
    pub startup_error: Option<String>,
}

impl RouterHealth {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bootstrapped: AtomicBool::new(false),
            dependencies_ready: AtomicBool::new(false),
            control_plane_synced: AtomicBool::new(false),
            wireguard_ready: AtomicBool::new(false),
            upstream_ca_ready: AtomicBool::new(false),
            startup_error: RwLock::const_new(None),
        }
    }

    pub fn mark_bootstrapped(&self) {
        self.bootstrapped.store(true, Ordering::Release);
    }

    pub fn mark_dependencies_ready(&self) {
        self.dependencies_ready.store(true, Ordering::Release);
    }

    pub fn mark_control_plane_synced(&self) {
        self.control_plane_synced.store(true, Ordering::Release);
    }

    pub fn mark_wireguard_ready(&self) {
        self.wireguard_ready.store(true, Ordering::Release);
    }

    pub fn mark_upstream_ca_ready(&self) {
        self.upstream_ca_ready.store(true, Ordering::Release);
    }

    pub async fn set_startup_error(&self, error: impl Into<String>) {
        *self.startup_error.write().await = Some(error.into());
    }

    pub async fn clear_startup_error(&self) {
        *self.startup_error.write().await = None;
    }

    pub async fn snapshot(&self) -> HealthSnapshot {
        let bootstrapped = self.bootstrapped.load(Ordering::Acquire);
        let dependencies_ready = self.dependencies_ready.load(Ordering::Acquire);
        let control_plane_synced = self.control_plane_synced.load(Ordering::Acquire);
        let wireguard_ready = self.wireguard_ready.load(Ordering::Acquire);
        let upstream_ca_ready = self.upstream_ca_ready.load(Ordering::Acquire);
        let startup_error = self.startup_error.read().await.clone();
        let ready = bootstrapped
            && dependencies_ready
            && control_plane_synced
            && wireguard_ready
            && upstream_ca_ready
            && startup_error.is_none();

        HealthSnapshot {
            live: true,
            ready,
            dependencies_ready,
            control_plane_synced,
            wireguard_ready,
            upstream_ca_ready,
            startup_error,
        }
    }
}

pub async fn write_snapshot_response(
    session: &mut Session,
    status: u16,
    snapshot: &HealthSnapshot,
) -> pingora::prelude::Result<bool> {
    let body = serde_json::to_string_pretty(snapshot).map_err(|e| {
        Error::because(
            ErrorType::HTTPStatus(500),
            "Failed to serialize health snapshot",
            e,
        )
    })?;

    let mut response = ResponseHeader::build(status, Some(body.len()))?;
    response.insert_header("Content-Type", "application/json")?;
    session
        .write_response_header(Box::new(response), true)
        .await?;
    session.write_response_body(Some(body.into()), true).await?;
    Ok(true)
}

pub async fn write_text_response(
    session: &mut Session,
    status: u16,
    body: &str,
) -> pingora::prelude::Result<bool> {
    let mut response = ResponseHeader::build(status, Some(body.len()))?;
    response.insert_header("Content-Type", "text/plain")?;
    session
        .write_response_header(Box::new(response), true)
        .await?;
    session
        .write_response_body(Some(body.to_string().into()), true)
        .await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::RouterHealth;

    #[tokio::test]
    async fn snapshot_reflects_state_transitions() {
        let health = RouterHealth::new();
        let snapshot = health.snapshot().await;
        assert!(!snapshot.ready);
        assert!(snapshot.live);

        health.mark_bootstrapped();
        health.mark_dependencies_ready();
        health.mark_control_plane_synced();
        health.mark_wireguard_ready();
        health.mark_upstream_ca_ready();

        let snapshot = health.snapshot().await;
        assert!(snapshot.ready);
        assert!(snapshot.dependencies_ready);
        assert!(snapshot.control_plane_synced);
    }

    #[tokio::test]
    async fn startup_error_can_be_cleared_after_recovery() {
        let health = RouterHealth::new();
        health.mark_bootstrapped();
        health.mark_dependencies_ready();
        health.mark_control_plane_synced();
        health.mark_wireguard_ready();
        health.mark_upstream_ca_ready();
        health.set_startup_error("temporary failure").await;

        let snapshot = health.snapshot().await;
        assert!(!snapshot.ready);
        assert_eq!(snapshot.startup_error.as_deref(), Some("temporary failure"));

        health.clear_startup_error().await;

        let snapshot = health.snapshot().await;
        assert!(snapshot.ready);
        assert_eq!(snapshot.startup_error, None);
    }
}
