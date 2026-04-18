use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct VmInfo {
    pub job_id: String,
    pub app_id: String,
    pub app_name: String,
    pub image: String,
    pub status: String,
    pub host_id: String,
    pub vm_id: String,
}

#[derive(Debug, Serialize)]
pub struct VmStatusResponse {
    pub job_id: String,
    pub status: String,
    pub host_id: String,
    pub vm_id: String,
    pub scheduled_at: i64,
    pub started_at: i64,
    pub stopped_at: i64,
    pub error_message: String,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

pub async fn list_vms(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
) -> impl IntoResponse {
    match crate::scheduler::connect(&state.scheduler_config).await {
        Ok(channel) => {
            let mut client = mikrom_proto::scheduler::SchedulerServiceClient::new(channel);
            let req = mikrom_proto::scheduler::ListAppsRequest {
                user_id: auth.user_id,
                status: None,
            };
            match client.list_apps(req).await {
                Ok(resp) => {
                    let vms: Vec<VmInfo> = resp
                        .into_inner()
                        .apps
                        .into_iter()
                        .map(|a| VmInfo {
                            job_id: a.job_id,
                            app_id: a.app_id,
                            app_name: a.app_name,
                            image: a.image,
                            status: crate::scheduler::status_name(a.status).to_string(),
                            host_id: a.host_id,
                            vm_id: a.vm_id,
                        })
                        .collect();
                    (StatusCode::OK, Json(vms)).into_response()
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorBody {
                        error: e.message().to_string(),
                    }),
                )
                    .into_response(),
            }
        }
        Err(msg) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorBody { error: msg }),
        )
            .into_response(),
    }
}

pub async fn get_vm_status(
    _auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    match crate::scheduler::connect(&state.scheduler_config).await {
        Ok(channel) => {
            let mut client = mikrom_proto::scheduler::SchedulerServiceClient::new(channel);
            let req = mikrom_proto::scheduler::AppStatusRequest { job_id };
            match client.get_app_status(req).await {
                Ok(resp) => {
                    let inner = resp.into_inner();
                    let vm = VmStatusResponse {
                        job_id: inner.job_id,
                        status: crate::scheduler::status_name(inner.status).to_string(),
                        host_id: inner.host_id,
                        vm_id: inner.vm_id,
                        scheduled_at: inner.scheduled_at,
                        started_at: inner.started_at,
                        stopped_at: inner.stopped_at,
                        error_message: inner.error_message,
                    };
                    (StatusCode::OK, Json(vm)).into_response()
                }
                Err(e) if e.code() == tonic::Code::NotFound => (
                    StatusCode::NOT_FOUND,
                    Json(ErrorBody {
                        error: "Job not found".to_string(),
                    }),
                )
                    .into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorBody {
                        error: e.message().to_string(),
                    }),
                )
                    .into_response(),
            }
        }
        Err(msg) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorBody { error: msg }),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::user_repository::{DbError, NewUser, User, UserRepository};
    use async_trait::async_trait;
    use axum::{body::Body, http::Request, routing::get};
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

    const TEST_SECRET: &str = "vms-test-secret";

    fn make_app(scheduler_addr: &str) -> axum::Router {
        let state = crate::AppState {
            user_repo: Arc::new(NoopRepo),
            scheduler_client: None,
            scheduler_config: crate::scheduler::SchedulerConfig {
                addr: scheduler_addr.to_string(),
                use_tls: false,
                certs_dir: None,
            },
            jwt_secret: TEST_SECRET.to_string(),
        };
        axum::Router::new()
            .route("/vms", get(list_vms))
            .route("/vms/{job_id}", get(get_vm_status))
            .with_state(state)
    }

    fn valid_token() -> String {
        crate::auth::jwt::create_token("uid-vms", "vms@example.com", TEST_SECRET).unwrap()
    }

    // ── serialization ──────────────────────────────────────────────────────────

    #[test]
    fn test_vm_info_serialization() {
        let vm = VmInfo {
            job_id: "job-1".to_string(),
            app_id: "app-1".to_string(),
            app_name: "my-app".to_string(),
            image: "nginx:latest".to_string(),
            status: "Scheduled".to_string(),
            host_id: "host-1".to_string(),
            vm_id: "vm-1".to_string(),
        };
        let json = serde_json::to_value(&vm).unwrap();
        assert_eq!(json["job_id"], "job-1");
        assert_eq!(json["status"], "Scheduled");
        assert_eq!(json["host_id"], "host-1");
    }

    #[test]
    fn test_vm_status_response_serialization() {
        let resp = VmStatusResponse {
            job_id: "job-2".to_string(),
            status: "Running".to_string(),
            host_id: "host-2".to_string(),
            vm_id: "vm-2".to_string(),
            scheduled_at: 1_700_000_000,
            started_at: 1_700_000_005,
            stopped_at: 0,
            error_message: String::new(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "Running");
        assert_eq!(json["scheduled_at"], 1_700_000_000_i64);
        assert_eq!(json["stopped_at"], 0);
    }

    #[test]
    fn test_vm_status_response_with_error_message() {
        let resp = VmStatusResponse {
            job_id: "job-3".to_string(),
            status: "Failed".to_string(),
            host_id: String::new(),
            vm_id: String::new(),
            scheduled_at: 0,
            started_at: 0,
            stopped_at: 0,
            error_message: "no workers available".to_string(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "Failed");
        assert_eq!(json["error_message"], "no workers available");
    }

    // ── GET /vms auth guard ────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_list_vms_without_token_returns_401() {
        let resp = make_app("http://127.0.0.1:59950")
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vms")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_get_vm_status_without_token_returns_401() {
        let resp = make_app("http://127.0.0.1:59951")
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vms/some-job-id")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // ── GET /vms scheduler unavailable ────────────────────────────────────────

    #[tokio::test]
    async fn test_list_vms_scheduler_unavailable_returns_503() {
        let resp = make_app("http://127.0.0.1:59952")
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vms")
                    .header("Authorization", format!("Bearer {}", valid_token()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_get_vm_status_scheduler_unavailable_returns_503() {
        let resp = make_app("http://127.0.0.1:59953")
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vms/any-job-id")
                    .header("Authorization", format!("Bearer {}", valid_token()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    // ── helper: start an in-process scheduler and return its port ─────────────

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

    async fn deploy_job(port: u16, user_id: &str) -> String {
        use mikrom_proto::scheduler::{AppConfig, DeployRequest, SchedulerServiceClient};
        let channel = tonic::transport::Channel::from_shared(format!("http://127.0.0.1:{port}"))
            .unwrap()
            .connect()
            .await
            .unwrap();
        let mut client = SchedulerServiceClient::new(channel);
        let resp = client
            .deploy_app(DeployRequest {
                app_id: uuid::Uuid::new_v4().to_string(),
                app_name: "test-app".to_string(),
                image: "nginx:latest".to_string(),
                config: Some(AppConfig {
                    vcpus: 1,
                    memory_mib: 256,
                    disk_mib: 1024,
                    env: Default::default(),
                }),
                user_id: user_id.to_string(),
            })
            .await
            .unwrap();
        resp.into_inner().job_id
    }

    // ── GET /vms with real in-process scheduler ────────────────────────────────

    #[tokio::test]
    async fn test_list_vms_returns_empty_array_when_no_jobs() {
        let port = start_scheduler().await;
        let resp = make_app(&format!("http://127.0.0.1:{port}"))
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vms")
                    .header("Authorization", format!("Bearer {}", valid_token()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(json.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_get_vm_status_not_found_returns_404() {
        let port = start_scheduler().await;
        let resp = make_app(&format!("http://127.0.0.1:{port}"))
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vms/nonexistent-job-id")
                    .header("Authorization", format!("Bearer {}", valid_token()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ── GET /vms happy path ───────────────────────────────────────────────────

    #[tokio::test]
    async fn test_list_vms_returns_deployed_jobs() {
        let port = start_scheduler().await;
        deploy_job(port, "uid-vms").await;
        let resp = make_app(&format!("http://127.0.0.1:{port}"))
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vms")
                    .header("Authorization", format!("Bearer {}", valid_token()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["app_name"], "test-app");
        assert_eq!(arr[0]["image"], "nginx:latest");
    }

    #[tokio::test]
    async fn test_list_vms_does_not_return_other_users_jobs() {
        let port = start_scheduler().await;
        deploy_job(port, "other-user").await;
        let resp = make_app(&format!("http://127.0.0.1:{port}"))
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vms")
                    .header("Authorization", format!("Bearer {}", valid_token()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(json.as_array().unwrap().is_empty());
    }

    // ── GET /vms/{job_id} happy path ──────────────────────────────────────────

    #[tokio::test]
    async fn test_get_vm_status_success_returns_job_details() {
        let port = start_scheduler().await;
        let job_id = deploy_job(port, "uid-vms").await;
        let resp = make_app(&format!("http://127.0.0.1:{port}"))
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/vms/{job_id}"))
                    .header("Authorization", format!("Bearer {}", valid_token()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["job_id"], job_id);
        assert!(json.get("status").is_some());
    }
}
