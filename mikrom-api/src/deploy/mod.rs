use axum::{Json, extract::State, response::IntoResponse};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct VolumeRequest {
    pub volume_id: String,
    pub size_mib: u64,
    pub read_only: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct DeployRequestBody {
    pub app_name: String,
    pub image: String,
    pub vcpus: Option<u32>,
    pub memory_mib: Option<u64>,
    pub disk_mib: Option<u64>,
    pub env: Option<std::collections::HashMap<String, String>>,
    pub volumes: Option<Vec<VolumeRequest>>,
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
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Json(payload): Json<DeployRequestBody>,
) -> impl IntoResponse {
    tracing::info!(
        user_id = %auth.user_id,
        app_name = %payload.app_name,
        image = %payload.image,
        "User requesting deployment"
    );
    let job_id = Uuid::new_v4().to_string();

    tracing::info!(
        "Deploy request: app={}, image={}, job_id={}",
        payload.app_name,
        payload.image,
        job_id
    );

    let vcpus = payload.vcpus.unwrap_or(1);
    let memory_mib = payload.memory_mib.unwrap_or(256);
    let disk_mib = payload.disk_mib.unwrap_or(1024);

    let response = match crate::scheduler::connect(&state.scheduler_config).await {
        Ok(channel) => {
            let mut client = mikrom_proto::scheduler::SchedulerServiceClient::new(channel);
            let req = mikrom_proto::scheduler::DeployRequest {
                app_id: Uuid::new_v4().to_string(),
                app_name: payload.app_name.clone(),
                image: payload.image.clone(),
                config: Some(mikrom_proto::scheduler::AppConfig {
                    vcpus,
                    memory_mib: memory_mib as u32,
                    disk_mib: disk_mib as u32,
                    env: payload.env.clone().unwrap_or_default(),
                    ip_address: String::new(),
                    gateway: String::new(),
                    mac_address: String::new(),
                    volumes: payload
                        .volumes
                        .as_ref()
                        .unwrap_or(&vec![])
                        .iter()
                        .map(|v| mikrom_proto::scheduler::Volume {
                            volume_id: v.volume_id.clone(),
                            size_mib: v.size_mib,
                            read_only: v.read_only.unwrap_or(false),
                        })
                        .collect(),
                }),

                user_id: auth.user_id,
            };

            match client.deploy_app(req).await {
                Ok(response) => {
                    let inner = response.into_inner();
                    DeployResponseBody {
                        job_id: inner.job_id,
                        status: crate::scheduler::status_name(inner.status).to_string(),
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
        Err(msg) => {
            tracing::error!("Failed to connect to scheduler: {}", msg);
            DeployResponseBody {
                job_id: job_id.clone(),
                status: "error".to_string(),
                host_id: None,
                vm_id: None,
                message: msg,
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

    fn minimal_payload() -> DeployRequestBody {
        DeployRequestBody {
            app_name: "test-app".to_string(),
            image: "alpine:latest".to_string(),
            vcpus: None,
            memory_mib: None,
            disk_mib: None,
            env: None,
            volumes: None,
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
        let req = minimal_payload();
        assert_eq!(req.vcpus.unwrap_or(1), 1);
    }

    #[test]
    fn test_memory_mib_default_is_256() {
        let req = minimal_payload();
        assert_eq!(req.memory_mib.unwrap_or(256), 256);
    }

    #[test]
    fn test_disk_mib_default_is_1024() {
        let req = minimal_payload();
        assert_eq!(req.disk_mib.unwrap_or(1024), 1024);
    }

    #[test]
    fn test_provided_vcpus_overrides_default() {
        let req = DeployRequestBody {
            vcpus: Some(8),
            ..minimal_payload()
        };
        assert_eq!(req.vcpus.unwrap_or(1), 8);
    }

    #[test]
    fn test_provided_memory_mib_overrides_default() {
        let req = DeployRequestBody {
            memory_mib: Some(4096),
            ..minimal_payload()
        };
        assert_eq!(req.memory_mib.unwrap_or(256), 4096);
    }

    #[test]
    fn test_provided_disk_mib_overrides_default() {
        let req = DeployRequestBody {
            disk_mib: Some(10240),
            ..minimal_payload()
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
        let req = minimal_payload();
        let s = format!("{:?}", req);
        assert!(s.contains("app_name"));
        assert!(s.contains("image"));
    }

    // ── deploy_app handler ───────────────────────────────────────────────────

    use crate::repositories::user_repository::{DbError, NewUser, User, UserRepository};
    use async_trait::async_trait;
    use axum::{body::Body, http::Request};
    use std::sync::Arc;
    use tower::ServiceExt;

    struct NoopRepo;
    #[async_trait]
    impl UserRepository for NoopRepo {
        async fn find_by_email(&self, _: &str) -> Result<Option<User>, DbError> {
            Ok(None)
        }
        async fn create(&self, _: NewUser) -> Result<sqlx::types::Uuid, DbError> {
            Ok(sqlx::types::Uuid::new_v4())
        }
        async fn count_by_email(&self, _: &str) -> Result<i64, DbError> {
            Ok(0)
        }
    }

    const TEST_JWT_SECRET: &str = "deploy-test-secret";

    fn make_deploy_app(scheduler_addr: &str) -> axum::Router {
        let state = crate::AppState {
            user_repo: Arc::new(NoopRepo),
            scheduler_client: None,
            scheduler_config: crate::scheduler::SchedulerConfig {
                addr: scheduler_addr.to_string(),
                use_tls: false,
                certs_dir: None,
            },
            jwt_secret: TEST_JWT_SECRET.to_string(),
        };
        axum::Router::new()
            .route("/deploy", axum::routing::post(deploy_app))
            .with_state(state)
    }

    fn make_deploy_app_tls(scheduler_addr: &str, certs_dir: &str) -> axum::Router {
        let state = crate::AppState {
            user_repo: Arc::new(NoopRepo),
            scheduler_client: None,
            scheduler_config: crate::scheduler::SchedulerConfig {
                addr: scheduler_addr.to_string(),
                use_tls: true,
                certs_dir: Some(certs_dir.to_string()),
            },
            jwt_secret: TEST_JWT_SECRET.to_string(),
        };
        axum::Router::new()
            .route("/deploy", axum::routing::post(deploy_app))
            .with_state(state)
    }

    fn valid_token() -> String {
        crate::auth::jwt::create_token("uid-test", "test@example.com", TEST_JWT_SECRET).unwrap()
    }

    fn deploy_body() -> Body {
        Body::from(r#"{"app_name":"test-app","image":"nginx:latest"}"#)
    }

    async fn post_deploy(app: axum::Router) -> serde_json::Value {
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/deploy")
                    .header("Content-Type", "application/json")
                    .header("Authorization", format!("Bearer {}", valid_token()))
                    .body(deploy_body())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn test_deploy_app_response_always_200_ok() {
        let json = post_deploy(make_deploy_app("http://127.0.0.1:59990")).await;
        assert!(json.get("job_id").is_some());
        assert!(json.get("status").is_some());
        assert!(json.get("message").is_some());
    }

    #[tokio::test]
    async fn test_deploy_app_response_job_id_is_non_empty_uuid() {
        let json = post_deploy(make_deploy_app("http://127.0.0.1:59991")).await;
        let job_id = json["job_id"].as_str().unwrap();
        assert_eq!(job_id.len(), 36, "job_id should be a UUID (36 chars)");
    }

    #[tokio::test]
    async fn test_deploy_app_scheduler_unreachable_returns_error_status() {
        let json = post_deploy(make_deploy_app("http://127.0.0.1:59992")).await;
        assert_eq!(json["status"], "error");
        let msg = json["message"].as_str().unwrap();
        assert!(!msg.is_empty(), "error message should be set");
    }

    #[tokio::test]
    async fn test_deploy_app_scheduler_unreachable_host_and_vm_are_null() {
        let json = post_deploy(make_deploy_app("http://127.0.0.1:59993")).await;
        assert!(json["host_id"].is_null());
        assert!(json["vm_id"].is_null());
    }

    #[tokio::test]
    async fn test_deploy_app_tls_cert_loading_failure_returns_error_message() {
        let json = post_deploy(make_deploy_app_tls(
            "http://127.0.0.1:59994",
            "/nonexistent-certs-dir-for-test",
        ))
        .await;
        assert_eq!(json["status"], "error");
        let msg = json["message"].as_str().unwrap().to_lowercase();
        assert!(
            msg.contains("tls") || msg.contains("cert") || msg.contains("failed"),
            "expected TLS error in message, got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn test_deploy_app_http_rewritten_to_https_when_use_tls_true() {
        // With use_tls=true and addr starting with http://, the handler rewrites it to https://.
        // TLS cert loading fails, confirming the rewrite happened.
        let json = post_deploy(make_deploy_app_tls(
            "http://127.0.0.1:59995",
            "/nonexistent-dir",
        ))
        .await;
        assert_eq!(json["status"], "error");
        let msg = json["message"].as_str().unwrap();
        assert!(
            msg.contains("TLS") || msg.contains("certificate") || msg.contains("Failed to load"),
            "expected TLS-related error; got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn test_deploy_app_default_resource_values_accepted() {
        let resp = make_deploy_app("http://127.0.0.1:59996")
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/deploy")
                    .header("Content-Type", "application/json")
                    .header("Authorization", format!("Bearer {}", valid_token()))
                    .body(Body::from(r#"{"app_name":"app","image":"alpine:latest"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_deploy_app_with_env_vars_in_payload() {
        let resp = make_deploy_app("http://127.0.0.1:59997")
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/deploy")
                    .header("Content-Type", "application/json")
                    .header("Authorization", format!("Bearer {}", valid_token()))
                    .body(Body::from(
                        r#"{"app_name":"app","image":"alpine","env":{"PORT":"8080","ENV":"prod"}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
    }

    // ── auth guard on /deploy ──────────────────────────────────────────────────

    #[tokio::test]
    async fn test_deploy_without_token_returns_401() {
        let resp = make_deploy_app("http://127.0.0.1:59998")
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/deploy")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"app_name":"app","image":"alpine:latest"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::UNAUTHORIZED);
    }

    // ── deploy_app handler — real in-process scheduler ────────────────────────

    async fn start_scheduler() -> u16 {
        use mikrom_scheduler::server::SchedulerServer;
        use std::net::SocketAddr;
        let port = {
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            l.local_addr().unwrap().port()
        };
        let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        tokio::spawn(async move {
            SchedulerServer::new(None)
                .unwrap()
                .serve(addr)
                .await
                .unwrap();
        });
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
                .await
                .is_ok()
            {
                break;
            }
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        port
    }

    #[tokio::test]
    async fn test_deploy_app_with_real_scheduler_covers_grpc_ok_branch() {
        let port = start_scheduler().await;
        // No workers → scheduler responds Ok(Failed). Covers the gRPC Ok branch.
        let json = post_deploy(make_deploy_app(&format!("http://127.0.0.1:{port}"))).await;
        assert!(!json["job_id"].as_str().unwrap().is_empty());
        // Status is scheduler-reported, not "error" (connection error).
        assert_ne!(json["status"].as_str().unwrap(), "error");
    }

    #[tokio::test]
    async fn test_deploy_app_with_real_scheduler_response_has_job_id_and_message() {
        let port = start_scheduler().await;
        let json = post_deploy(make_deploy_app(&format!("http://127.0.0.1:{port}"))).await;
        assert!(!json["message"].as_str().unwrap().is_empty());
        assert!(json["host_id"].is_null()); // no workers → empty host_id filtered to null
    }

    #[tokio::test]
    async fn test_deploy_with_invalid_token_returns_401() {
        let resp = make_deploy_app("http://127.0.0.1:59999")
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/deploy")
                    .header("Content-Type", "application/json")
                    .header("Authorization", "Bearer this.is.invalid")
                    .body(Body::from(r#"{"app_name":"app","image":"alpine:latest"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::UNAUTHORIZED);
    }
}
