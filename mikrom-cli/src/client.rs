use anyhow::bail;
use serde::Deserialize;
use std::collections::HashMap;

pub struct MikromClient {
    http: reqwest::Client,
    base_url: String,
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

#[derive(Debug, Deserialize)]
pub struct RegisterResponse {
    pub message: String,
    pub user_id: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginResponse {
    pub token: String,
}

#[derive(Debug, Deserialize)]
pub struct DeployResponse {
    pub job_id: String,
    pub status: String,
    pub host_id: Option<String>,
    pub vm_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct VmInfo {
    pub job_id: String,
    pub app_id: String,
    pub app_name: String,
    pub image: String,
    pub status: String,
    pub host_id: String,
    pub vm_id: String,
}

#[derive(Debug, Deserialize)]
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

#[derive(Deserialize)]
struct ErrorResponse {
    error: String,
}

impl MikromClient {
    pub fn new(base_url: String, token: Option<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url,
            token,
        }
    }

    pub async fn health(&self) -> anyhow::Result<HealthResponse> {
        let resp = self
            .http
            .get(format!("{}/health", self.base_url))
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json().await?)
    }

    pub async fn register(&self, email: &str, password: &str) -> anyhow::Result<RegisterResponse> {
        let resp = self
            .http
            .post(format!("{}/auth/register", self.base_url))
            .json(&serde_json::json!({ "email": email, "password": password }))
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(resp.json().await?)
        } else {
            let status = resp.status().as_u16();
            let err: ErrorResponse = resp.json().await?;
            bail!("{} (HTTP {})", err.error, status);
        }
    }

    pub async fn login(&self, email: &str, password: &str) -> anyhow::Result<LoginResponse> {
        let resp = self
            .http
            .post(format!("{}/auth/login", self.base_url))
            .json(&serde_json::json!({ "email": email, "password": password }))
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(resp.json().await?)
        } else {
            let status = resp.status().as_u16();
            let err: ErrorResponse = resp.json().await?;
            bail!("{} (HTTP {})", err.error, status);
        }
    }

    pub async fn deploy(
        &self,
        app_name: &str,
        image: &str,
        vcpus: Option<u32>,
        memory_mib: Option<u64>,
        disk_mib: Option<u64>,
        env: HashMap<String, String>,
    ) -> anyhow::Result<DeployResponse> {
        let mut body = serde_json::json!({
            "app_name": app_name,
            "image": image,
        });

        if let Some(v) = vcpus {
            body["vcpus"] = serde_json::json!(v);
        }
        if let Some(m) = memory_mib {
            body["memory_mib"] = serde_json::json!(m);
        }
        if let Some(d) = disk_mib {
            body["disk_mib"] = serde_json::json!(d);
        }
        if !env.is_empty() {
            body["env"] = serde_json::json!(env);
        }

        let mut req = self
            .http
            .post(format!("{}/deploy", self.base_url))
            .json(&body);

        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }

        let resp = req.send().await?.error_for_status()?;
        Ok(resp.json().await?)
    }

    pub async fn list_vms(&self) -> anyhow::Result<Vec<VmInfo>> {
        let mut req = self.http.get(format!("{}/vms", self.base_url));
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await?;
        if resp.status().is_success() {
            Ok(resp.json().await?)
        } else {
            let status = resp.status().as_u16();
            let err: ErrorResponse = resp.json().await?;
            bail!("{} (HTTP {})", err.error, status);
        }
    }

    pub async fn get_vm(&self, job_id: &str) -> anyhow::Result<VmStatusResponse> {
        let mut req = self.http.get(format!("{}/vms/{}", self.base_url, job_id));
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await?;
        if resp.status().is_success() {
            Ok(resp.json().await?)
        } else {
            let status = resp.status().as_u16();
            let err: ErrorResponse = resp.json().await?;
            bail!("{} (HTTP {})", err.error, status);
        }
    }

    pub async fn stop_vm(&self, job_id: &str) -> anyhow::Result<StopVmResponse> {
        let mut req = self
            .http
            .delete(format!("{}/vms/{}", self.base_url, job_id));
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await?;
        if resp.status().is_success() {
            Ok(resp.json().await?)
        } else {
            let status = resp.status().as_u16();
            let err: ErrorResponse = resp.json().await?;
            bail!("{} (HTTP {})", err.error, status);
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct StopVmResponse {
    pub success: bool,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

    /// Matcher that passes only when the request has no Authorization header.
    struct NoAuthHeader;
    impl Match for NoAuthHeader {
        fn matches(&self, req: &Request) -> bool {
            !req.headers.contains_key("authorization")
        }
    }

    // ── health ───────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_health_returns_status_and_version() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "ok",
                "version": "0.1.0"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let resp = client.health().await.unwrap();
        assert_eq!(resp.status, "ok");
        assert_eq!(resp.version, "0.1.0");
    }

    #[tokio::test]
    async fn test_health_server_error_returns_err() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.health().await.is_err());
    }

    // ── register ─────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_register_success_returns_user_id() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/register"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "message": "User registered successfully",
                "user_id": "550e8400-e29b-41d4-a716-446655440000"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let resp = client
            .register("user@example.com", "password123")
            .await
            .unwrap();
        assert_eq!(resp.message, "User registered successfully");
        assert_eq!(resp.user_id, "550e8400-e29b-41d4-a716-446655440000");
    }

    #[tokio::test]
    async fn test_register_conflict_includes_status_in_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/register"))
            .respond_with(ResponseTemplate::new(409).set_body_json(serde_json::json!({
                "error": "Email already registered"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let err = client
            .register("dup@example.com", "pass")
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Email already registered"));
        assert!(msg.contains("409"));
    }

    #[tokio::test]
    async fn test_register_bad_request_includes_status_in_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/register"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "Password must be at least 8 characters"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let err = client
            .register("user@example.com", "short")
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Password must be at least 8 characters"));
        assert!(msg.contains("400"));
    }

    // ── login ────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_login_success_returns_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/login"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "eyJhbGciOiJIUzI1NiJ9.payload.sig"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let resp = client
            .login("user@example.com", "password123")
            .await
            .unwrap();
        assert_eq!(resp.token, "eyJhbGciOiJIUzI1NiJ9.payload.sig");
    }

    #[tokio::test]
    async fn test_login_unauthorized_includes_status_in_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/login"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": "Invalid credentials"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let err = client.login("user@example.com", "wrong").await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Invalid credentials"));
        assert!(msg.contains("401"));
    }

    // ── deploy ───────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_deploy_success_returns_response_fields() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/deploy"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "job_id": "job-abc",
                "status": "Scheduled",
                "host_id": "host-1",
                "vm_id": "vm-xyz",
                "message": "Application scheduled"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let resp = client
            .deploy("my-app", "nginx:latest", None, None, None, HashMap::new())
            .await
            .unwrap();
        assert_eq!(resp.job_id, "job-abc");
        assert_eq!(resp.status, "Scheduled");
        assert_eq!(resp.host_id.as_deref(), Some("host-1"));
        assert_eq!(resp.vm_id.as_deref(), Some("vm-xyz"));
        assert_eq!(resp.message, "Application scheduled");
    }

    #[tokio::test]
    async fn test_deploy_response_with_null_host_and_vm() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/deploy"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "job_id": "job-err",
                "status": "error",
                "host_id": null,
                "vm_id": null,
                "message": "Scheduler unavailable"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let resp = client
            .deploy("app", "img", None, None, None, HashMap::new())
            .await
            .unwrap();
        assert!(resp.host_id.is_none());
        assert!(resp.vm_id.is_none());
    }

    #[tokio::test]
    async fn test_deploy_server_error_returns_err() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/deploy"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(
            client
                .deploy("app", "img", None, None, None, HashMap::new())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_deploy_sends_bearer_token_when_set() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/deploy"))
            .and(header("authorization", "Bearer my-secret-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "job_id": "j", "status": "ok", "host_id": null,
                "vm_id": null, "message": "ok"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("my-secret-token".to_string()));
        assert!(
            client
                .deploy("app", "img", None, None, None, HashMap::new())
                .await
                .is_ok()
        );
    }

    // ── Malformed JSON responses ──────────────────────────────────────────────

    #[tokio::test]
    async fn test_health_malformed_json_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200).set_body_string("this is not json"))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.health().await.is_err());
    }

    #[tokio::test]
    async fn test_register_malformed_json_on_success_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/register"))
            .respond_with(ResponseTemplate::new(201).set_body_string("{bad json"))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.register("a@b.com", "password123").await.is_err());
    }

    #[tokio::test]
    async fn test_register_malformed_json_on_error_response_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/register"))
            .respond_with(ResponseTemplate::new(409).set_body_string("not json"))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.register("a@b.com", "pass").await.is_err());
    }

    #[tokio::test]
    async fn test_login_malformed_json_on_success_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/login"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{{invalid"))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.login("a@b.com", "password123").await.is_err());
    }

    #[tokio::test]
    async fn test_login_malformed_json_on_error_response_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/login"))
            .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.login("a@b.com", "wrong").await.is_err());
    }

    #[tokio::test]
    async fn test_deploy_malformed_json_on_success_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/deploy"))
            .respond_with(ResponseTemplate::new(200).set_body_string("definitely not json"))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(
            client
                .deploy("app", "img", None, None, None, HashMap::new())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_health_empty_body_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![]))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.health().await.is_err());
    }

    // ── Server unreachable ────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_health_server_unreachable_returns_error() {
        // Port 59970 is highly unlikely to have anything listening.
        let client = MikromClient::new("http://127.0.0.1:59970".to_string(), None);
        assert!(client.health().await.is_err());
    }

    #[tokio::test]
    async fn test_register_server_unreachable_returns_error() {
        let client = MikromClient::new("http://127.0.0.1:59971".to_string(), None);
        assert!(client.register("a@b.com", "password123").await.is_err());
    }

    #[tokio::test]
    async fn test_login_server_unreachable_returns_error() {
        let client = MikromClient::new("http://127.0.0.1:59972".to_string(), None);
        assert!(client.login("a@b.com", "password123").await.is_err());
    }

    #[tokio::test]
    async fn test_deploy_server_unreachable_returns_error() {
        let client = MikromClient::new("http://127.0.0.1:59973".to_string(), None);
        assert!(
            client
                .deploy("app", "img", None, None, None, HashMap::new())
                .await
                .is_err()
        );
    }

    // ── Missing required fields in success response ───────────────────────────

    #[tokio::test]
    async fn test_health_response_missing_field_returns_error() {
        let server = MockServer::start().await;
        // Response has "status" but no "version" field.
        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"status": "ok"})),
            )
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        // Deserialization should fail: "version" is a required field.
        assert!(client.health().await.is_err());
    }

    #[tokio::test]
    async fn test_deploy_no_auth_header_without_token() {
        let server = MockServer::start().await;
        // This mock only matches if the Authorization header is absent.
        // If the client sends it, the mock won't match and the request will
        // get a 404, causing the test to fail.
        Mock::given(method("POST"))
            .and(path("/deploy"))
            .and(NoAuthHeader)
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "job_id": "j", "status": "ok", "host_id": null,
                "vm_id": null, "message": "ok"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(
            client
                .deploy("app", "img", None, None, None, HashMap::new())
                .await
                .is_ok()
        );
    }

    // ── list_vms ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_list_vms_success_returns_vm_list() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "job_id": "job-1",
                    "app_id": "app-1",
                    "app_name": "my-app",
                    "image": "nginx:latest",
                    "status": "Scheduled",
                    "host_id": "host-1",
                    "vm_id": "vm-abc"
                }
            ])))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("tok".to_string()));
        let vms = client.list_vms().await.unwrap();
        assert_eq!(vms.len(), 1);
        assert_eq!(vms[0].job_id, "job-1");
        assert_eq!(vms[0].status, "Scheduled");
        assert_eq!(vms[0].vm_id, "vm-abc");
    }

    #[tokio::test]
    async fn test_list_vms_empty_returns_empty_vec() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("tok".to_string()));
        let vms = client.list_vms().await.unwrap();
        assert!(vms.is_empty());
    }

    #[tokio::test]
    async fn test_list_vms_sends_bearer_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms"))
            .and(header("authorization", "Bearer mytoken"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("mytoken".to_string()));
        assert!(client.list_vms().await.is_ok());
    }

    #[tokio::test]
    async fn test_list_vms_401_returns_error_with_message() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_json(serde_json::json!({"error": "Invalid or expired token"})),
            )
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let err = client.list_vms().await.unwrap_err();
        assert!(err.to_string().contains("Invalid or expired token"));
        assert!(err.to_string().contains("401"));
    }

    #[tokio::test]
    async fn test_list_vms_503_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms"))
            .respond_with(
                ResponseTemplate::new(503)
                    .set_body_json(serde_json::json!({"error": "Scheduler unavailable"})),
            )
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.list_vms().await.is_err());
    }

    #[tokio::test]
    async fn test_list_vms_server_unreachable_returns_error() {
        let client = MikromClient::new("http://127.0.0.1:59960".to_string(), None);
        assert!(client.list_vms().await.is_err());
    }

    // ── get_vm ────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_get_vm_success_returns_status_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms/job-abc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "job_id": "job-abc",
                "status": "Running",
                "host_id": "host-1",
                "vm_id": "vm-xyz",
                "scheduled_at": 1_700_000_000_i64,
                "started_at": 1_700_000_005_i64,
                "stopped_at": 0_i64,
                "error_message": ""
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("tok".to_string()));
        let vm = client.get_vm("job-abc").await.unwrap();
        assert_eq!(vm.job_id, "job-abc");
        assert_eq!(vm.status, "Running");
        assert_eq!(vm.host_id, "host-1");
        assert_eq!(vm.vm_id, "vm-xyz");
        assert_eq!(vm.scheduled_at, 1_700_000_000);
        assert_eq!(vm.stopped_at, 0);
    }

    #[tokio::test]
    async fn test_get_vm_sends_bearer_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms/job-1"))
            .and(header("authorization", "Bearer secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "job_id": "job-1", "status": "Scheduled",
                "host_id": "h", "vm_id": "v",
                "scheduled_at": 0_i64, "started_at": 0_i64,
                "stopped_at": 0_i64, "error_message": ""
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("secret".to_string()));
        assert!(client.get_vm("job-1").await.is_ok());
    }

    #[tokio::test]
    async fn test_get_vm_404_returns_error_with_message() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms/ghost"))
            .respond_with(
                ResponseTemplate::new(404)
                    .set_body_json(serde_json::json!({"error": "Job not found"})),
            )
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let err = client.get_vm("ghost").await.unwrap_err();
        assert!(err.to_string().contains("Job not found"));
        assert!(err.to_string().contains("404"));
    }

    #[tokio::test]
    async fn test_get_vm_server_unreachable_returns_error() {
        let client = MikromClient::new("http://127.0.0.1:59961".to_string(), None);
        assert!(client.get_vm("job-1").await.is_err());
    }

    #[tokio::test]
    async fn test_get_vm_builds_correct_url_with_job_id() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms/my-special-job-id"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "job_id": "my-special-job-id", "status": "Failed",
                "host_id": "", "vm_id": "",
                "scheduled_at": 0_i64, "started_at": 0_i64,
                "stopped_at": 0_i64, "error_message": "spawn error"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let vm = client.get_vm("my-special-job-id").await.unwrap();
        assert_eq!(vm.error_message, "spawn error");
    }

    // ── stop_vm ───────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_stop_vm_success_returns_response() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/vms/job-abc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "message": "Application cancelled"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("tok".to_string()));
        let resp = client.stop_vm("job-abc").await.unwrap();
        assert!(resp.success);
        assert_eq!(resp.message, "Application cancelled");
    }

    #[tokio::test]
    async fn test_stop_vm_404_returns_error_with_message() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/vms/ghost"))
            .respond_with(
                ResponseTemplate::new(404)
                    .set_body_json(serde_json::json!({"error": "Job not found"})),
            )
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let err = client.stop_vm("ghost").await.unwrap_err();
        assert!(err.to_string().contains("Job not found"));
        assert!(err.to_string().contains("404"));
    }

    #[tokio::test]
    async fn test_stop_vm_sends_bearer_token() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/vms/job-1"))
            .and(header("authorization", "Bearer secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "message": "cancelled"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("secret".to_string()));
        assert!(client.stop_vm("job-1").await.is_ok());
    }

    #[tokio::test]
    async fn test_stop_vm_server_unreachable_returns_error() {
        let client = MikromClient::new("http://127.0.0.1:59962".to_string(), None);
        assert!(client.stop_vm("job-1").await.is_err());
    }
}
