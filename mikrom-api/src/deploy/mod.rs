use axum::{Json, extract::State, response::IntoResponse};
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
        payload.app_name,
        payload.image,
        job_id
    );

    let use_tls = std::env::var("USE_TLS")
        .map(|v| v == "true")
        .unwrap_or(false);

    let mut scheduler_uri =
        std::env::var("SCHEDULER_ADDR").unwrap_or_else(|_| "http://127.0.0.1:5002".to_string());

    if use_tls && scheduler_uri.starts_with("http://") {
        scheduler_uri = scheduler_uri.replacen("http://", "https://", 1);
    }

    let vcpus = payload.vcpus.unwrap_or(1);
    let memory_mib = payload.memory_mib.unwrap_or(256);
    let disk_mib = payload.disk_mib.unwrap_or(1024);

    let endpoint_result: Result<Endpoint, String> = (|| {
        let ep =
            Endpoint::new(scheduler_uri).map_err(|e| format!("Invalid scheduler URI: {}", e))?;

        if use_tls {
            let certs_dir = std::env::var("CERTS_DIR").unwrap_or_else(|_| "/certs/api".to_string());
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
        response.job_id,
        response.status,
        response.message
    );

    Json(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_req() -> DeployRequestBody {
        DeployRequestBody {
            app_name: "my-app".to_string(),
            image: "nginx:latest".to_string(),
            vcpus: None,
            memory_mib: None,
            disk_mib: None,
            env: None,
        }
    }

    // ── DeployRequestBody deserialization ────────────────────────────────────

    #[test]
    fn test_deploy_request_full_deserialization() {
        let json = r#"{
            "app_name": "my-service",
            "image": "nginx:1.25",
            "vcpus": 4,
            "memory_mib": 2048,
            "disk_mib": 8192,
            "env": {"PORT": "8080", "ENV": "prod"}
        }"#;
        let req: DeployRequestBody = serde_json::from_str(json).unwrap();
        assert_eq!(req.app_name, "my-service");
        assert_eq!(req.image, "nginx:1.25");
        assert_eq!(req.vcpus, Some(4));
        assert_eq!(req.memory_mib, Some(2048));
        assert_eq!(req.disk_mib, Some(8192));
        let env = req.env.unwrap();
        assert_eq!(env.get("PORT").unwrap(), "8080");
        assert_eq!(env.get("ENV").unwrap(), "prod");
    }

    #[test]
    fn test_deploy_request_minimal_required_fields_only() {
        let json = r#"{"app_name": "app", "image": "alpine:3"}"#;
        let req: DeployRequestBody = serde_json::from_str(json).unwrap();
        assert_eq!(req.app_name, "app");
        assert_eq!(req.image, "alpine:3");
        assert!(req.vcpus.is_none());
        assert!(req.memory_mib.is_none());
        assert!(req.disk_mib.is_none());
        assert!(req.env.is_none());
    }

    #[test]
    fn test_deploy_request_missing_app_name_fails() {
        let json = r#"{"image": "nginx"}"#;
        assert!(serde_json::from_str::<DeployRequestBody>(json).is_err());
    }

    #[test]
    fn test_deploy_request_missing_image_fails() {
        let json = r#"{"app_name": "app"}"#;
        assert!(serde_json::from_str::<DeployRequestBody>(json).is_err());
    }

    #[test]
    fn test_deploy_request_empty_env_map() {
        let json = r#"{"app_name": "app", "image": "nginx", "env": {}}"#;
        let req: DeployRequestBody = serde_json::from_str(json).unwrap();
        assert!(req.env.unwrap().is_empty());
    }

    // ── Default values ───────────────────────────────────────────────────────

    #[test]
    fn test_vcpus_default_is_1() {
        let req = minimal_req();
        assert_eq!(req.vcpus.unwrap_or(1), 1);
    }

    #[test]
    fn test_memory_mib_default_is_256() {
        let req = minimal_req();
        assert_eq!(req.memory_mib.unwrap_or(256), 256);
    }

    #[test]
    fn test_disk_mib_default_is_1024() {
        let req = minimal_req();
        assert_eq!(req.disk_mib.unwrap_or(1024), 1024);
    }

    #[test]
    fn test_provided_vcpus_overrides_default() {
        let req = DeployRequestBody {
            vcpus: Some(8),
            ..minimal_req()
        };
        assert_eq!(req.vcpus.unwrap_or(1), 8);
    }

    #[test]
    fn test_provided_memory_mib_overrides_default() {
        let req = DeployRequestBody {
            memory_mib: Some(4096),
            ..minimal_req()
        };
        assert_eq!(req.memory_mib.unwrap_or(256), 4096);
    }

    #[test]
    fn test_provided_disk_mib_overrides_default() {
        let req = DeployRequestBody {
            disk_mib: Some(10240),
            ..minimal_req()
        };
        assert_eq!(req.disk_mib.unwrap_or(1024), 10240);
    }

    // ── DeployResponseBody serialization ─────────────────────────────────────

    #[test]
    fn test_deploy_response_serialization_all_fields() {
        let resp = DeployResponseBody {
            job_id: "job-abc".to_string(),
            status: "1".to_string(),
            host_id: Some("h1".to_string()),
            vm_id: Some("vm-xyz".to_string()),
            message: "Application scheduled".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["job_id"], "job-abc");
        assert_eq!(v["status"], "1");
        assert_eq!(v["host_id"], "h1");
        assert_eq!(v["vm_id"], "vm-xyz");
        assert_eq!(v["message"], "Application scheduled");
    }

    #[test]
    fn test_deploy_response_none_fields_serialize_as_null() {
        let resp = DeployResponseBody {
            job_id: "job-err".to_string(),
            status: "error".to_string(),
            host_id: None,
            vm_id: None,
            message: "Scheduler unavailable".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["host_id"], serde_json::Value::Null);
        assert_eq!(v["vm_id"], serde_json::Value::Null);
        assert_eq!(v["message"], "Scheduler unavailable");
    }

    #[test]
    fn test_deploy_response_is_debug_formattable() {
        let resp = DeployResponseBody {
            job_id: "j".to_string(),
            status: "ok".to_string(),
            host_id: None,
            vm_id: None,
            message: "ok".to_string(),
        };
        let s = format!("{:?}", resp);
        assert!(s.contains("job_id"));
        assert!(s.contains("status"));
    }

    #[test]
    fn test_deploy_request_is_debug_formattable() {
        let req = minimal_req();
        let s = format!("{:?}", req);
        assert!(s.contains("app_name"));
        assert!(s.contains("image"));
    }
}
