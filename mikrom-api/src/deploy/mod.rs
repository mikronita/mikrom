use axum::{Json, extract::State, response::IntoResponse};
use serde::{Deserialize, Serialize};
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
    auth: crate::auth::AuthUser,
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

    let vcpus = payload.vcpus.unwrap_or(1);
    let memory_mib = payload.memory_mib.unwrap_or(256);
    let disk_mib = payload.disk_mib.unwrap_or(1024);

    let response = match crate::scheduler::connect().await {
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

    // ── deploy_app handler ───────────────────────────────────────────────────
    //
    // Env-var-mutating tests are serialized with ENV_MUTEX to avoid data races
    // when cargo test runs them in parallel within the same process.

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

    // tokio::sync::Mutex is async-aware: safe to hold across await points.
    static ENV_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn make_deploy_app() -> axum::Router {
        let state = crate::AppState {
            user_repo: Arc::new(NoopRepo),
            scheduler_client: None,
        };
        axum::Router::new()
            .route("/deploy", axum::routing::post(deploy_app))
            .with_state(state)
    }

    const TEST_JWT_SECRET: &str = "deploy-test-secret";

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
        let _guard = ENV_MUTEX.lock().await;
        // Point to a definitely-unreachable address so no real scheduler needed.
        unsafe {
            std::env::set_var("SCHEDULER_ADDR", "http://127.0.0.1:59990");
            std::env::set_var("JWT_SECRET", TEST_JWT_SECRET);
        }
        let json = post_deploy(make_deploy_app()).await;
        assert!(json.get("job_id").is_some());
        assert!(json.get("status").is_some());
        assert!(json.get("message").is_some());
        unsafe {
            std::env::remove_var("SCHEDULER_ADDR");
            std::env::remove_var("JWT_SECRET");
        }
    }

    #[tokio::test]
    async fn test_deploy_app_response_job_id_is_non_empty_uuid() {
        let _guard = ENV_MUTEX.lock().await;
        unsafe {
            std::env::set_var("SCHEDULER_ADDR", "http://127.0.0.1:59991");
            std::env::set_var("JWT_SECRET", TEST_JWT_SECRET);
        }
        let json = post_deploy(make_deploy_app()).await;
        let job_id = json["job_id"].as_str().unwrap();
        assert_eq!(job_id.len(), 36, "job_id should be a UUID (36 chars)");
        unsafe {
            std::env::remove_var("SCHEDULER_ADDR");
            std::env::remove_var("JWT_SECRET");
        }
    }

    #[tokio::test]
    async fn test_deploy_app_scheduler_unreachable_returns_error_status() {
        let _guard = ENV_MUTEX.lock().await;
        unsafe {
            std::env::set_var("SCHEDULER_ADDR", "http://127.0.0.1:59992");
            std::env::set_var("JWT_SECRET", TEST_JWT_SECRET);
        }
        let json = post_deploy(make_deploy_app()).await;
        assert_eq!(json["status"], "error");
        let msg = json["message"].as_str().unwrap();
        assert!(!msg.is_empty(), "error message should be set");
        unsafe {
            std::env::remove_var("SCHEDULER_ADDR");
            std::env::remove_var("JWT_SECRET");
        }
    }

    #[tokio::test]
    async fn test_deploy_app_scheduler_unreachable_host_and_vm_are_null() {
        let _guard = ENV_MUTEX.lock().await;
        unsafe {
            std::env::set_var("SCHEDULER_ADDR", "http://127.0.0.1:59993");
            std::env::set_var("JWT_SECRET", TEST_JWT_SECRET);
        }
        let json = post_deploy(make_deploy_app()).await;
        assert!(json["host_id"].is_null());
        assert!(json["vm_id"].is_null());
        unsafe {
            std::env::remove_var("SCHEDULER_ADDR");
            std::env::remove_var("JWT_SECRET");
        }
    }

    #[tokio::test]
    async fn test_deploy_app_tls_cert_loading_failure_returns_error_message() {
        let _guard = ENV_MUTEX.lock().await;
        unsafe {
            std::env::set_var("USE_TLS", "true");
            std::env::set_var("SCHEDULER_ADDR", "http://127.0.0.1:59994");
            std::env::set_var("CERTS_DIR", "/nonexistent-certs-dir-for-test");
            std::env::set_var("JWT_SECRET", TEST_JWT_SECRET);
        }
        let json = post_deploy(make_deploy_app()).await;
        assert_eq!(json["status"], "error");
        let msg = json["message"].as_str().unwrap().to_lowercase();
        assert!(
            msg.contains("tls") || msg.contains("cert") || msg.contains("failed"),
            "expected TLS error in message, got: {}",
            msg
        );
        unsafe {
            std::env::remove_var("USE_TLS");
            std::env::remove_var("SCHEDULER_ADDR");
            std::env::remove_var("CERTS_DIR");
            std::env::remove_var("JWT_SECRET");
        }
    }

    #[tokio::test]
    async fn test_deploy_app_http_rewritten_to_https_when_use_tls_true() {
        // When USE_TLS=true and SCHEDULER_ADDR starts with http://,
        // the handler rewrites it to https:// before building the endpoint.
        // TLS cert loading will fail (no certs dir), proving the rewrite happened
        // because a plain HTTP endpoint would produce a different error.
        let _guard = ENV_MUTEX.lock().await;
        unsafe {
            std::env::set_var("USE_TLS", "true");
            std::env::set_var("SCHEDULER_ADDR", "http://127.0.0.1:59995");
            std::env::set_var("CERTS_DIR", "/nonexistent-dir");
            std::env::set_var("JWT_SECRET", TEST_JWT_SECRET);
        }
        let json = post_deploy(make_deploy_app()).await;
        // The error must come from TLS cert loading, not from an HTTP connection attempt,
        // which confirms the URI was rewritten to https://.
        assert_eq!(json["status"], "error");
        let msg = json["message"].as_str().unwrap();
        assert!(
            msg.contains("TLS") || msg.contains("certificate") || msg.contains("Failed to load"),
            "expected TLS-related error; got: {}",
            msg
        );
        unsafe {
            std::env::remove_var("USE_TLS");
            std::env::remove_var("SCHEDULER_ADDR");
            std::env::remove_var("CERTS_DIR");
            std::env::remove_var("JWT_SECRET");
        }
    }

    #[tokio::test]
    async fn test_deploy_app_default_resource_values_accepted() {
        let _guard = ENV_MUTEX.lock().await;
        unsafe {
            std::env::set_var("SCHEDULER_ADDR", "http://127.0.0.1:59996");
            std::env::set_var("JWT_SECRET", TEST_JWT_SECRET);
        }
        // Payload with no vcpus/memory/disk — handler must apply defaults without panicking.
        let resp = make_deploy_app()
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
        unsafe {
            std::env::remove_var("SCHEDULER_ADDR");
            std::env::remove_var("JWT_SECRET");
        }
    }

    #[tokio::test]
    async fn test_deploy_app_with_env_vars_in_payload() {
        let _guard = ENV_MUTEX.lock().await;
        unsafe {
            std::env::set_var("SCHEDULER_ADDR", "http://127.0.0.1:59997");
            std::env::set_var("JWT_SECRET", TEST_JWT_SECRET);
        }
        let resp = make_deploy_app()
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
        unsafe {
            std::env::remove_var("SCHEDULER_ADDR");
            std::env::remove_var("JWT_SECRET");
        }
    }

    // ── auth guard on /deploy ──────────────────────────────────────────────────

    #[tokio::test]
    async fn test_deploy_without_token_returns_401() {
        let _guard = ENV_MUTEX.lock().await;
        unsafe {
            std::env::set_var("SCHEDULER_ADDR", "http://127.0.0.1:59998");
            std::env::set_var("JWT_SECRET", TEST_JWT_SECRET);
        }
        let resp = make_deploy_app()
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
        unsafe {
            std::env::remove_var("SCHEDULER_ADDR");
            std::env::remove_var("JWT_SECRET");
        }
    }

    #[tokio::test]
    async fn test_deploy_with_invalid_token_returns_401() {
        let _guard = ENV_MUTEX.lock().await;
        unsafe {
            std::env::set_var("SCHEDULER_ADDR", "http://127.0.0.1:59999");
            std::env::set_var("JWT_SECRET", TEST_JWT_SECRET);
        }
        let resp = make_deploy_app()
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
        unsafe {
            std::env::remove_var("SCHEDULER_ADDR");
            std::env::remove_var("JWT_SECRET");
        }
    }
}
