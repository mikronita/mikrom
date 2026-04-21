use tonic::transport::Channel;

#[derive(Clone, Debug)]
pub struct SchedulerConfig {
    pub addr: String,
    pub use_tls: bool,
    pub certs_dir: Option<String>,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            addr: "http://127.0.0.1:5002".to_string(),
            use_tls: false,
            certs_dir: None,
        }
    }
}

impl SchedulerConfig {
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            addr: std::env::var("SCHEDULER_ADDR")
                .unwrap_or_else(|_| "http://127.0.0.1:5002".to_string()),
            use_tls: std::env::var("USE_TLS").is_ok_and(|v| v == "true"),
            certs_dir: std::env::var("CERTS_DIR").ok(),
        }
    }
}

/// Build and connect a gRPC channel to the scheduler.
pub async fn connect(config: &SchedulerConfig) -> Result<Channel, String> {
    let mut uri = config.addr.clone();

    if config.use_tls && uri.starts_with("http://") {
        uri = uri.replacen("http://", "https://", 1);
    }

    let ep =
        tonic::transport::Endpoint::new(uri).map_err(|e| format!("Invalid scheduler URI: {e}"))?;

    let ep = if config.use_tls {
        let certs_dir = config.certs_dir.as_deref().unwrap_or("/certs/api");
        let certs = mikrom_proto::tls::ServiceCerts::load(certs_dir)
            .map_err(|e| format!("Failed to load TLS certificates: {e}"))?;
        ep.tls_config(certs.client_tls_config("mikrom-scheduler"))
            .map_err(|e| format!("TLS config error: {e}"))?
    } else {
        ep
    };

    ep.connect()
        .await
        .map_err(|e| format!("Scheduler unavailable: {e}"))
}

#[must_use]
pub fn status_name(code: i32) -> &'static str {
    use mikrom_proto::scheduler::DeployStatus;
    match DeployStatus::try_from(code).unwrap_or(DeployStatus::Unspecified) {
        DeployStatus::Unspecified => "Unspecified",
        DeployStatus::Pending => "Pending",
        DeployStatus::Scheduled => "Scheduled",
        DeployStatus::Running => "Running",
        DeployStatus::Failed => "Failed",
        DeployStatus::Cancelled => "Cancelled",
        DeployStatus::Paused => "Paused",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_name_all_variants() {
        assert_eq!(status_name(0), "Unspecified");
        assert_eq!(status_name(1), "Pending");
        assert_eq!(status_name(2), "Scheduled");
        assert_eq!(status_name(3), "Running");
        assert_eq!(status_name(4), "Failed");
        assert_eq!(status_name(5), "Cancelled");
    }

    #[test]
    fn test_status_name_unknown_code_returns_unspecified() {
        assert_eq!(status_name(99), "Unspecified");
        assert_eq!(status_name(-1), "Unspecified");
    }

    #[tokio::test]
    async fn test_connect_returns_error_when_scheduler_unreachable() {
        let config = SchedulerConfig {
            addr: "http://127.0.0.1:59940".to_string(),
            use_tls: false,
            certs_dir: None,
        };
        let result = connect(&config).await;
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(
            msg.contains("Scheduler unavailable") || msg.contains("unavailable"),
            "unexpected error: {msg}"
        );
    }

    #[tokio::test]
    async fn test_connect_tls_returns_error() {
        // With use_tls=true the endpoint is rewritten to https:// and a TLS
        // handshake is attempted. With nothing listening the connection must fail.
        let config = SchedulerConfig {
            addr: "http://127.0.0.1:59941".to_string(),
            use_tls: true,
            certs_dir: Some("/nonexistent-scheduler-test-certs".to_string()),
        };
        let result = connect(&config).await;
        assert!(result.is_err(), "expected error, got ok");
    }
}
