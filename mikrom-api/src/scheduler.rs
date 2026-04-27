use std::time::Duration;
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

    let mut ep =
        tonic::transport::Endpoint::new(uri).map_err(|e| format!("Invalid scheduler URI: {e}"))?;

    // Add reasonable timeouts
    ep = ep
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(5));

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

#[async_trait::async_trait]
pub trait Scheduler: Send + Sync {
    async fn pause_app(&self, job_id: String, user_id: String) -> Result<bool, String>;
    async fn resume_app(&self, job_id: String, user_id: String) -> Result<bool, String>;
    async fn delete_app(&self, job_id: String, user_id: String) -> Result<bool, String>;
}

pub struct NatsScheduler {
    pub client: async_nats::Client,
}

#[async_trait::async_trait]
impl Scheduler for NatsScheduler {
    async fn pause_app(&self, job_id: String, user_id: String) -> Result<bool, String> {
        use mikrom_proto::scheduler::{PauseRequest, PauseResponse};
        use prost::Message;
        let nats_req = PauseRequest { job_id, user_id };
        let mut buf = Vec::new();
        nats_req.encode(&mut buf).map_err(|e| e.to_string())?;

        let response = self
            .client
            .request("mikrom.scheduler.pause_app", buf.into())
            .await
            .map_err(|e| e.to_string())?;

        let inner = PauseResponse::decode(&response.payload[..]).map_err(|e| e.to_string())?;

        Ok(inner.success)
    }

    async fn resume_app(&self, job_id: String, user_id: String) -> Result<bool, String> {
        use mikrom_proto::scheduler::{ResumeRequest, ResumeResponse};
        use prost::Message;
        let nats_req = ResumeRequest { job_id, user_id };
        let mut buf = Vec::new();
        nats_req.encode(&mut buf).map_err(|e| e.to_string())?;

        let response = self
            .client
            .request("mikrom.scheduler.resume_app", buf.into())
            .await
            .map_err(|e| e.to_string())?;

        let inner = ResumeResponse::decode(&response.payload[..]).map_err(|e| e.to_string())?;

        Ok(inner.success)
    }

    async fn delete_app(&self, job_id: String, user_id: String) -> Result<bool, String> {
        use mikrom_proto::scheduler::{DeleteAppRequest, DeleteAppResponse};
        use prost::Message;
        let nats_req = DeleteAppRequest { job_id, user_id };
        let mut buf = Vec::new();
        nats_req.encode(&mut buf).map_err(|e| e.to_string())?;

        let response = self
            .client
            .request("mikrom.scheduler.delete_app", buf.into())
            .await
            .map_err(|e| e.to_string())?;

        let inner = DeleteAppResponse::decode(&response.payload[..]).map_err(|e| e.to_string())?;

        Ok(inner.success)
    }
}

pub struct TonicScheduler {
    pub client: mikrom_proto::scheduler::SchedulerServiceClient<Channel>,
}

#[async_trait::async_trait]
impl Scheduler for TonicScheduler {
    async fn pause_app(&self, job_id: String, user_id: String) -> Result<bool, String> {
        let mut client = self.client.clone();
        let resp = client
            .pause_app(mikrom_proto::scheduler::PauseRequest { job_id, user_id })
            .await
            .map_err(|e| e.to_string())?;
        Ok(resp.into_inner().success)
    }

    async fn resume_app(&self, job_id: String, user_id: String) -> Result<bool, String> {
        let mut client = self.client.clone();
        let resp = client
            .resume_app(mikrom_proto::scheduler::ResumeRequest { job_id, user_id })
            .await
            .map_err(|e| e.to_string())?;
        Ok(resp.into_inner().success)
    }

    async fn delete_app(&self, job_id: String, user_id: String) -> Result<bool, String> {
        let mut client = self.client.clone();
        let resp = client
            .delete_app(mikrom_proto::scheduler::DeleteAppRequest { job_id, user_id })
            .await
            .map_err(|e| e.to_string())?;
        Ok(resp.into_inner().success)
    }
}

mockall::mock! {
    pub Scheduler {}
    #[async_trait::async_trait]
    impl Scheduler for Scheduler {
        async fn pause_app(&self, job_id: String, user_id: String) -> Result<bool, String>;
        async fn resume_app(&self, job_id: String, user_id: String) -> Result<bool, String>;
        async fn delete_app(&self, job_id: String, user_id: String) -> Result<bool, String>;
    }
}

#[must_use]
pub fn status_name(code: i32) -> &'static str {
    use mikrom_proto::scheduler::DeployStatus;
    match DeployStatus::try_from(code).unwrap_or(DeployStatus::Unspecified) {
        DeployStatus::Unspecified => "UNKNOWN",
        DeployStatus::Pending => "PENDING",
        DeployStatus::Scheduled => "SCHEDULED",
        DeployStatus::Running => "RUNNING",
        DeployStatus::Failed => "FAILED",
        DeployStatus::Cancelled => "CANCELLED",
        DeployStatus::Paused => "STOPPED",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_name_all_variants() {
        assert_eq!(status_name(0), "UNKNOWN");
        assert_eq!(status_name(1), "PENDING");
        assert_eq!(status_name(2), "SCHEDULED");
        assert_eq!(status_name(3), "RUNNING");
        assert_eq!(status_name(4), "FAILED");
        assert_eq!(status_name(5), "CANCELLED");
        assert_eq!(status_name(6), "STOPPED");
    }

    #[test]
    fn test_status_name_unknown_code_returns_unspecified() {
        assert_eq!(status_name(99), "UNKNOWN");
        assert_eq!(status_name(-1), "UNKNOWN");
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
