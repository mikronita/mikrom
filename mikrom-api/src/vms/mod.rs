use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{
        IntoResponse,
        sse::{Event, Sse},
    },
};
use serde::Serialize;
use tokio_stream::StreamExt;

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
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
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
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    match crate::scheduler::connect(&state.scheduler_config).await {
        Ok(channel) => {
            let mut client = mikrom_proto::scheduler::SchedulerServiceClient::new(channel);
            let req = mikrom_proto::scheduler::AppStatusRequest {
                job_id,
                user_id: auth.user_id,
            };
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
                        cpu_usage: 0.0,    // Placeholder
                        ram_used_bytes: 0, // Placeholder
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

pub async fn get_vm_logs(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    match crate::scheduler::connect(&state.scheduler_config).await {
        Ok(channel) => {
            let mut client = mikrom_proto::scheduler::SchedulerServiceClient::new(channel);
            let req = mikrom_proto::scheduler::GetLogsRequest {
                job_id,
                user_id: auth.user_id,
                follow: true,
            };
            match client.get_app_logs(req).await {
                Ok(resp) => {
                    let stream = resp.into_inner().map(|res| match res {
                        Ok(log) => {
                            let data = serde_json::json!({
                                "line": log.line,
                                "timestamp": log.timestamp,
                            })
                            .to_string();
                            Ok::<Event, std::convert::Infallible>(Event::default().data(data))
                        }
                        Err(e) => {
                            let data = serde_json::json!({
                                "line": format!("Error: {}", e),
                                "timestamp": chrono::Utc::now().timestamp(),
                            })
                            .to_string();
                            Ok::<Event, std::convert::Infallible>(Event::default().data(data))
                        }
                    });
                    Sse::new(stream).into_response()
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

pub async fn stop_vm(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    match crate::scheduler::connect(&state.scheduler_config).await {
        Ok(channel) => {
            let mut client = mikrom_proto::scheduler::SchedulerServiceClient::new(channel);
            let req = mikrom_proto::scheduler::CancelRequest {
                job_id: job_id.clone(),
                user_id: auth.user_id,
            };
            match client.cancel_app(req).await {
                Ok(resp) => {
                    let inner = resp.into_inner();
                    if inner.success {
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "success": true,
                                "message": inner.message
                            })),
                        )
                            .into_response()
                    } else {
                        (
                            StatusCode::NOT_FOUND,
                            Json(ErrorBody {
                                error: inner.message,
                            }),
                        )
                            .into_response()
                    }
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

pub async fn delete_vm(
    auth: crate::auth::AuthUser,
    State(state): State<crate::AppState>,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    match crate::scheduler::connect(&state.scheduler_config).await {
        Ok(channel) => {
            let mut client = mikrom_proto::scheduler::SchedulerServiceClient::new(channel);
            let req = mikrom_proto::scheduler::DeleteAppRequest {
                job_id,
                user_id: auth.user_id,
            };
            match client.delete_app(req).await {
                Ok(resp) => {
                    let inner = resp.into_inner();
                    if inner.success {
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "success": true,
                                "message": inner.message
                            })),
                        )
                            .into_response()
                    } else {
                        (
                            StatusCode::NOT_FOUND,
                            Json(ErrorBody {
                                error: inner.message,
                            }),
                        )
                            .into_response()
                    }
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
            .route("/vms/{job_id}/logs", get(get_vm_logs))
            .route("/vms/{job_id}", axum::routing::delete(stop_vm))
            .route("/vms/{job_id}/delete", axum::routing::delete(delete_vm))
            .with_state(state)
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_vm_logs_returns_sse_stream() {
        let port = start_scheduler().await;

        // Register a worker first so deployment succeeds and has a host
        let channel = crate::scheduler::connect(&crate::scheduler::SchedulerConfig {
            addr: format!("http://127.0.0.1:{port}"),
            use_tls: false,
            certs_dir: None,
        })
        .await
        .unwrap();
        let mut client = mikrom_proto::scheduler::SchedulerServiceClient::new(channel);

        client
            .register_worker(mikrom_proto::scheduler::RegisterWorkerRequest {
                host_id: "h1".to_string(),
                hostname: "host1".to_string(),
                ip_address: "127.0.0.1".to_string(),
                agent_port: 5003,
            })
            .await
            .unwrap();

        client
            .report_metrics(mikrom_proto::scheduler::ReportMetricsRequest {
                host_id: "h1".to_string(),
                cpu_usage: 0.1,
                ram_used_bytes: 0,
                ram_total_bytes: 4 * 1024 * 1024 * 1024,
                disk_used_bytes: 0,
                disk_total_bytes: 100 * 1024 * 1024 * 1024,
                apps_count: 0,
                timestamp: 0,
                load_avg_1: 0.0,
                load_avg_5: 0.0,
                load_avg_15: 0.0,
                vms: std::collections::HashMap::new(),
            })
            .await
            .unwrap();

        // Use the deploy_app RPC to actually create the job in the scheduler
        let deploy_req = mikrom_proto::scheduler::DeployRequest {
            app_id: "app-sse".to_string(),
            app_name: "app-sse".to_string(),
            image: "img".to_string(),
            config: Some(mikrom_proto::scheduler::AppConfig {
                vcpus: 1,
                memory_mib: 128,
                disk_mib: 512,
                env: std::collections::HashMap::new(),
                ip_address: String::new(),
                gateway: String::new(),
                mac_address: String::new(),
                volumes: vec![],
            }),
            user_id: "uid-vms".to_string(),
        };

        let deploy_resp = client.deploy_app(deploy_req).await.unwrap().into_inner();
        let job_id = deploy_resp.job_id;

        let resp = make_app(&format!("http://127.0.0.1:{port}"))
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/vms/{job_id}/logs"))
                    .header("Authorization", format!("Bearer {}", valid_token()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers()["content-type"], "text/event-stream");
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
            cpu_usage: 0.1,
            ram_used_bytes: 1024,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "Running");
        assert_eq!(json["scheduled_at"], 1_700_000_000_i64);
        assert_eq!(json["stopped_at"], 0);
        let cpu = json["cpu_usage"].as_f64().unwrap();
        assert!((cpu - 0.1).abs() < 0.0001);
        assert_eq!(json["ram_used_bytes"], 1024);
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
            cpu_usage: 0.0,
            ram_used_bytes: 0,
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
                    ip_address: String::new(),
                    gateway: String::new(),
                    mac_address: String::new(),
                    volumes: vec![],
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

    // ── DELETE /vms/{job_id} auth guard ───────────────────────────────────────

    #[tokio::test]
    async fn test_stop_vm_without_token_returns_401() {
        let resp = make_app("http://127.0.0.1:59980")
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/vms/some-job-id")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // ── DELETE /vms/{job_id} scheduler unavailable ────────────────────────────

    #[tokio::test]
    async fn test_stop_vm_scheduler_unavailable_returns_503() {
        let resp = make_app("http://127.0.0.1:59981")
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/vms/any-job-id")
                    .header("Authorization", format!("Bearer {}", valid_token()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    // ── DELETE /vms/{job_id} job not found ────────────────────────────────────

    #[tokio::test]
    async fn test_stop_vm_not_found_returns_404() {
        let port = start_scheduler().await;
        let resp = make_app(&format!("http://127.0.0.1:{port}"))
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/vms/nonexistent-job-id")
                    .header("Authorization", format!("Bearer {}", valid_token()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ── DELETE /vms/{job_id} happy path ───────────────────────────────────────

    #[tokio::test]
    async fn test_stop_vm_success_returns_200() {
        let port = start_scheduler().await;
        let job_id = deploy_job(port, "uid-vms").await;
        let resp = make_app(&format!("http://127.0.0.1:{port}"))
            .oneshot(
                Request::builder()
                    .method("DELETE")
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
        assert_eq!(json["success"], true);
    }

    #[tokio::test]
    async fn test_stop_vm_response_has_message_field() {
        let port = start_scheduler().await;
        let job_id = deploy_job(port, "uid-vms").await;
        let resp = make_app(&format!("http://127.0.0.1:{port}"))
            .oneshot(
                Request::builder()
                    .method("DELETE")
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
        assert!(json.get("message").is_some());
    }

    // ── DELETE /vms/{job_id}/delete auth guard ────────────────────────────────

    #[tokio::test]
    async fn test_delete_vm_without_token_returns_401() {
        let resp = make_app("http://127.0.0.1:59990")
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/vms/some-job-id/delete")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // ── DELETE /vms/{job_id}/delete happy path ───────────────────────────────

    #[tokio::test]
    async fn test_delete_vm_success_returns_200() {
        let port = start_scheduler().await;
        let job_id = deploy_job(port, "uid-vms").await;
        let resp = make_app(&format!("http://127.0.0.1:{port}"))
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/vms/{job_id}/delete"))
                    .header("Authorization", format!("Bearer {}", valid_token()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);
    }
}
