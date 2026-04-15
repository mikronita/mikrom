use axum::{
    extract::State,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use tonic::transport::Endpoint;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct DeployRequestBody {
    pub app_name: String,
    pub image: String,
    pub vcpus: Option<u32>,
    pub memory_mib: Option<u64>,
    pub disk_mib: Option<u64>,
    pub env: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Serialize)]
pub struct DeployResponseBody {
    pub job_id: String,
    pub status: String,
    pub host_id: Option<String>,
    pub vm_id: Option<String>,
    pub message: String,
}

pub async fn deploy_app(
    State(_state): State<crate::AppState>,
    Json(payload): Json<DeployRequestBody>,
) -> impl IntoResponse {
    let job_id = Uuid::new_v4().to_string();

    tracing::info!(
        "Deploy request: app={}, image={}, job_id={}",
        payload.app_name, payload.image, job_id
    );

    let use_tls = std::env::var("USE_TLS")
        .map(|v| v == "true")
        .unwrap_or(false);

    let mut scheduler_uri = std::env::var("SCHEDULER_ADDR")
        .unwrap_or_else(|_| "http://127.0.0.1:5002".to_string());

    if use_tls && scheduler_uri.starts_with("http://") {
        scheduler_uri = scheduler_uri.replacen("http://", "https://", 1);
    }

    let vcpus      = payload.vcpus.unwrap_or(1);
    let memory_mib = payload.memory_mib.unwrap_or(256);
    let disk_mib   = payload.disk_mib.unwrap_or(1024);

    let endpoint_result: Result<Endpoint, String> = (|| {
        let ep = Endpoint::new(scheduler_uri)
            .map_err(|e| format!("Invalid scheduler URI: {}", e))?;

        if use_tls {
            let certs_dir = std::env::var("CERTS_DIR")
                .unwrap_or_else(|_| "/certs/api".to_string());
            let certs = mikrom_proto::tls::ServiceCerts::load(&certs_dir)
                .map_err(|e| format!("Failed to load TLS certificates: {}", e))?;
            ep.tls_config(certs.client_tls_config("mikrom-scheduler"))
                .map_err(|e| format!("TLS config error: {}", e))
        } else {
            Ok(ep)
        }
    })();

    let endpoint = match endpoint_result {
        Ok(ep) => ep,
        Err(msg) => {
            return Json(DeployResponseBody {
                job_id,
                status: "error".to_string(),
                host_id: None,
                vm_id: None,
                message: msg,
            });
        }
    };

    let response = match endpoint.connect().await {
        Ok(channel) => {
            let mut client = mikrom_proto::scheduler::SchedulerServiceClient::new(channel);
            let req = mikrom_proto::scheduler::DeployRequest {
                app_id: Uuid::new_v4().to_string(),
                app_name: payload.app_name.clone(),
                image: payload.image.clone(),
                config: Some(mikrom_proto::scheduler::AppConfig {
                    vcpus,
                    memory_mib,
                    disk_mib,
                    env: payload.env.clone().unwrap_or_default(),
                }),
                user_id: "default".to_string(),
            };

            match client.deploy_app(req).await {
                Ok(response) => {
                    let inner = response.into_inner();
                    DeployResponseBody {
                        job_id: inner.job_id,
                        status: format!("{:?}", inner.status),
                        host_id: Some(inner.host_id).filter(|s| !s.is_empty()),
                        vm_id: Some(inner.vm_id).filter(|s| !s.is_empty()),
                        message: inner.message,
                    }
                }
                Err(status) => DeployResponseBody {
                    job_id: job_id.clone(),
                    status: "error".to_string(),
                    host_id: None,
                    vm_id: None,
                    message: status.message().to_string(),
                },
            }
        }
        Err(e) => {
            tracing::error!("Failed to connect to scheduler: {}", e);
            DeployResponseBody {
                job_id: job_id.clone(),
                status: "error".to_string(),
                host_id: None,
                vm_id: None,
                message: format!("Scheduler unavailable: {}", e),
            }
        }
    };

    tracing::info!(
        "Deployment {} - status: {}, message: {}",
        response.job_id, response.status, response.message
    );

    Json(response)
}
