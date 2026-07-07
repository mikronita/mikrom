use pingora::prelude::*;
use serde::Serialize;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RouterHealthState {
    Booting,
    Degraded,
    Ready,
    ShuttingDown,
    Fatal,
}

impl RouterHealthState {
    const fn as_u8(self) -> u8 {
        match self {
            Self::Booting => 0,
            Self::Degraded => 1,
            Self::Ready => 2,
            Self::ShuttingDown => 3,
            Self::Fatal => 4,
        }
    }

    const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Degraded,
            2 => Self::Ready,
            3 => Self::ShuttingDown,
            4 => Self::Fatal,
            _ => Self::Booting,
        }
    }

    #[must_use]
    const fn is_live(self) -> bool {
        !matches!(self, Self::Fatal | Self::ShuttingDown)
    }

    #[must_use]
    const fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }
}

pub struct RouterHealth {
    state: AtomicU8,
    bootstrapped: AtomicBool,
    dependencies_ready: AtomicBool,
    control_plane_synced: AtomicBool,
    wireguard_ready: AtomicBool,
    upstream_ca_ready: AtomicBool,
    shutting_down: AtomicBool,
    startup_error: Mutex<Option<String>>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct HealthSnapshot {
    pub live: bool,
    pub ready: bool,
    pub state: RouterHealthState,
    pub dependencies_ready: bool,
    pub control_plane_synced: bool,
    pub wireguard_ready: bool,
    pub upstream_ca_ready: bool,
    pub startup_error: Option<String>,
}

impl Default for RouterHealth {
    fn default() -> Self {
        Self::new()
    }
}

impl RouterHealth {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: AtomicU8::new(RouterHealthState::Booting.as_u8()),
            bootstrapped: AtomicBool::new(false),
            dependencies_ready: AtomicBool::new(false),
            control_plane_synced: AtomicBool::new(false),
            wireguard_ready: AtomicBool::new(false),
            upstream_ca_ready: AtomicBool::new(false),
            shutting_down: AtomicBool::new(false),
            startup_error: Mutex::new(None),
        }
    }

    fn update_state(&self) {
        let state = if self.shutting_down.load(Ordering::Acquire) {
            RouterHealthState::ShuttingDown
        } else {
            let bootstrapped = self.bootstrapped.load(Ordering::Acquire);
            let dependencies_ready = self.dependencies_ready.load(Ordering::Acquire);
            let control_plane_synced = self.control_plane_synced.load(Ordering::Acquire);
            let wireguard_ready = self.wireguard_ready.load(Ordering::Acquire);
            let upstream_ca_ready = self.upstream_ca_ready.load(Ordering::Acquire);
            let startup_error = self.startup_error.lock().unwrap_or_else(|e| e.into_inner());

            if !bootstrapped && startup_error.is_some() {
                RouterHealthState::Fatal
            } else if bootstrapped
                && dependencies_ready
                && control_plane_synced
                && wireguard_ready
                && upstream_ca_ready
                && startup_error.is_none()
            {
                RouterHealthState::Ready
            } else if bootstrapped && startup_error.is_some() {
                RouterHealthState::Degraded
            } else {
                RouterHealthState::Booting
            }
        };

        self.state.store(state.as_u8(), Ordering::Release);
    }

    pub fn mark_bootstrapped(&self) {
        self.bootstrapped.store(true, Ordering::Release);
        self.update_state();
    }

    pub fn mark_dependencies_ready(&self) {
        self.dependencies_ready.store(true, Ordering::Release);
        self.update_state();
    }

    pub fn mark_control_plane_synced(&self) {
        self.control_plane_synced.store(true, Ordering::Release);
        self.update_state();
    }

    pub fn mark_wireguard_ready(&self) {
        self.wireguard_ready.store(true, Ordering::Release);
        self.update_state();
    }

    #[must_use]
    pub fn is_wireguard_ready(&self) -> bool {
        self.wireguard_ready.load(Ordering::Acquire)
    }

    pub fn mark_upstream_ca_ready(&self) {
        self.upstream_ca_ready.store(true, Ordering::Release);
        self.update_state();
    }

    pub fn mark_shutting_down(&self) {
        self.shutting_down.store(true, Ordering::Release);
        self.update_state();
    }

    pub fn set_startup_error(&self, error: impl Into<String>) {
        *self.startup_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(error.into());
        self.update_state();
    }

    pub fn clear_startup_error(&self) {
        *self.startup_error.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.update_state();
    }

    pub fn snapshot(&self) -> HealthSnapshot {
        self.update_state();

        let state = RouterHealthState::from_u8(self.state.load(Ordering::Acquire));
        let dependencies_ready = self.dependencies_ready.load(Ordering::Acquire);
        let control_plane_synced = self.control_plane_synced.load(Ordering::Acquire);
        let wireguard_ready = self.wireguard_ready.load(Ordering::Acquire);
        let upstream_ca_ready = self.upstream_ca_ready.load(Ordering::Acquire);
        let startup_error = self
            .startup_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        HealthSnapshot {
            live: state.is_live(),
            ready: state.is_ready(),
            state,
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
    crate::application::proxy::set_router_server_header(&mut response)?;
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
    crate::application::proxy::set_router_server_header(&mut response)?;
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
    use super::{RouterHealth, RouterHealthState};

    #[tokio::test]
    async fn snapshot_reflects_state_transitions() {
        let health = RouterHealth::new();
        let snapshot = health.snapshot();
        assert!(!snapshot.ready);
        assert!(snapshot.live);
        assert_eq!(snapshot.state, RouterHealthState::Booting);

        health.mark_bootstrapped();
        health.mark_dependencies_ready();
        health.mark_control_plane_synced();
        health.mark_wireguard_ready();
        health.mark_upstream_ca_ready();

        let snapshot = health.snapshot();
        assert!(snapshot.ready);
        assert!(snapshot.dependencies_ready);
        assert!(snapshot.control_plane_synced);
        assert_eq!(snapshot.state, RouterHealthState::Ready);
    }

    #[tokio::test]
    async fn startup_error_can_be_cleared_after_recovery() {
        let health = RouterHealth::new();
        health.mark_bootstrapped();
        health.mark_dependencies_ready();
        health.mark_control_plane_synced();
        health.mark_wireguard_ready();
        health.mark_upstream_ca_ready();
        health.set_startup_error("temporary failure");

        let snapshot = health.snapshot();
        assert!(!snapshot.ready);
        assert_eq!(snapshot.state, RouterHealthState::Degraded);
        assert_eq!(snapshot.startup_error.as_deref(), Some("temporary failure"));

        health.clear_startup_error();

        let snapshot = health.snapshot();
        assert!(snapshot.ready);
        assert_eq!(snapshot.state, RouterHealthState::Ready);
        assert_eq!(snapshot.startup_error, None);
    }

    #[tokio::test]
    async fn startup_error_before_bootstrap_is_fatal() {
        let health = RouterHealth::new();
        health.set_startup_error("fatal failure");

        let snapshot = health.snapshot();
        assert!(!snapshot.live);
        assert!(!snapshot.ready);
        assert_eq!(snapshot.state, RouterHealthState::Fatal);
        assert_eq!(snapshot.startup_error.as_deref(), Some("fatal failure"));
    }
}
