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
