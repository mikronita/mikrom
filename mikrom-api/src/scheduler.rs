use tonic::transport::Channel;

/// Build and connect a gRPC channel to the scheduler, respecting USE_TLS / CERTS_DIR.
pub async fn connect() -> Result<Channel, String> {
    let use_tls = std::env::var("USE_TLS")
        .map(|v| v == "true")
        .unwrap_or(false);

    let mut uri =
        std::env::var("SCHEDULER_ADDR").unwrap_or_else(|_| "http://127.0.0.1:5002".to_string());

    if use_tls && uri.starts_with("http://") {
        uri = uri.replacen("http://", "https://", 1);
    }

    let ep =
        tonic::transport::Endpoint::new(uri).map_err(|e| format!("Invalid scheduler URI: {e}"))?;

    let ep = if use_tls {
        let certs_dir = std::env::var("CERTS_DIR").unwrap_or_else(|_| "/certs/api".to_string());
        let certs = mikrom_proto::tls::ServiceCerts::load(&certs_dir)
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

pub fn status_name(code: i32) -> &'static str {
    use mikrom_proto::scheduler::DeployStatus;
    match DeployStatus::try_from(code).unwrap_or(DeployStatus::Unspecified) {
        DeployStatus::Unspecified => "Unspecified",
        DeployStatus::Pending => "Pending",
        DeployStatus::Scheduled => "Scheduled",
        DeployStatus::Running => "Running",
        DeployStatus::Failed => "Failed",
        DeployStatus::Cancelled => "Cancelled",
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
        unsafe { std::env::set_var("SCHEDULER_ADDR", "http://127.0.0.1:59940") };
        unsafe { std::env::remove_var("USE_TLS") };
        let result = connect().await;
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(
            msg.contains("Scheduler unavailable") || msg.contains("unavailable"),
            "unexpected error: {msg}"
        );
        unsafe { std::env::remove_var("SCHEDULER_ADDR") };
    }

    #[tokio::test]
    async fn test_connect_tls_returns_error() {
        // With USE_TLS=true the endpoint is rewritten to https:// and a TLS
        // handshake is attempted. With nothing listening the connection must fail.
        unsafe { std::env::set_var("USE_TLS", "true") };
        unsafe { std::env::set_var("SCHEDULER_ADDR", "http://127.0.0.1:59941") };
        unsafe { std::env::set_var("CERTS_DIR", "/nonexistent-scheduler-test-certs") };
        let result = connect().await;
        assert!(result.is_err(), "expected error, got ok");
        unsafe { std::env::remove_var("USE_TLS") };
        unsafe { std::env::remove_var("SCHEDULER_ADDR") };
        unsafe { std::env::remove_var("CERTS_DIR") };
    }
}
