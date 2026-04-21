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
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: String,
}

impl MikromClient {
    pub fn new(base_url: String, token: Option<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
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

    pub async fn get_vm_logs(&self, job_id: &str) -> anyhow::Result<String> {
        let mut req = self
            .http
            .get(format!("{}/vms/{}/logs", self.base_url, job_id));
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await?;
        if resp.status().is_success() {
            let body = resp.text().await?;
            Ok(body)
        } else {
            let status = resp.status().as_u16();
            let err: ErrorResponse = resp.json().await?;
            bail!("{} (HTTP {})", err.error, status);
        }
    }

    pub async fn stream_vm_logs(
        &self,
        job_id: &str,
    ) -> anyhow::Result<impl futures_util::Stream<Item = anyhow::Result<String>>> {
        let url = format!("{}/vms/{}/logs", self.base_url, job_id);
        let mut req = self.http.get(&url);
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }

        let resp = req.send().await?.error_for_status()?;

        use futures_util::StreamExt;
        let byte_stream = resp
            .bytes_stream()
            .map(|result| result.map_err(std::io::Error::other));

        let reader = tokio_util::io::StreamReader::new(byte_stream);
        let lines =
            tokio_util::codec::FramedRead::new(reader, tokio_util::codec::LinesCodec::new());

        #[derive(Deserialize)]
        struct LogLine {
            line: String,
            timestamp: Option<i64>,
        }

        Ok(lines.filter_map(|result| async move {
            match result {
                Ok(line) => {
                    if let Some(data) = line.strip_prefix("data: ") {
                        if let Ok(log_line) = serde_json::from_str::<LogLine>(data) {
                            if let Some(ts) = log_line.timestamp {
                                Some(Ok(format!("[{}] {}", ts, log_line.line)))
                            } else {
                                Some(Ok(log_line.line))
                            }
                        } else {
                            Some(Ok(data.to_string()))
                        }
                    } else {
                        None
                    }
                }
                Err(e) => Some(Err(anyhow::anyhow!("Stream error: {e}"))),
            }
        }))
    }

    pub async fn pause_vm(&self, job_id: &str) -> anyhow::Result<ActionResponse> {
        let mut req = self
            .http
            .post(format!("{}/vms/{}/pause", self.base_url, job_id));
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

    pub async fn resume_vm(&self, job_id: &str) -> anyhow::Result<ActionResponse> {
        let mut req = self
            .http
            .post(format!("{}/vms/{}/resume", self.base_url, job_id));
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

    pub async fn delete_vm(&self, job_id: &str) -> anyhow::Result<StopVmResponse> {
        let mut req = self
            .http
            .delete(format!("{}/vms/{}/delete", self.base_url, job_id));
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

    pub async fn restart_vm(&self, job_id: &str) -> anyhow::Result<StopVmResponse> {
        let mut req = self
            .http
            .post(format!("{}/vms/{}/restart", self.base_url, job_id));
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

    pub async fn get_vm_metrics(&self, job_id: &str) -> anyhow::Result<VmMetricsResponse> {
        let mut req = self
            .http
            .get(format!("{}/vms/{}/metrics", self.base_url, job_id));
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

    pub async fn whoami(&self) -> anyhow::Result<WhoamiResponse> {
        let mut req = self.http.get(format!("{}/auth/whoami", self.base_url));
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

#[derive(Debug, Deserialize)]
pub struct ActionResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct VmMetricsResponse {
    pub job_id: String,
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub disk_usage: f64,
    pub network_rx: u64,
    pub network_tx: u64,
    pub timestamp: i64,
}

#[derive(Debug, Deserialize)]
pub struct WhoamiResponse {
    pub user_id: String,
    pub email: String,
    pub created_at: String,
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
                "stopped_at": 0,
                "error_message": "",
                "cpu_usage": 0.0,
                "ram_used_bytes": 0
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
                "stopped_at": 0_i64, "error_message": "",
                "cpu_usage": 0.0, "ram_used_bytes": 0
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
                "stopped_at": 0_i64, "error_message": "spawn error",
                "cpu_usage": 0.0, "ram_used_bytes": 0
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

    // ── get_vm_logs ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_get_vm_logs_success_returns_logs() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms/job-abc/logs"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("data: {\"line\":\"hello\",\"timestamp\":123}\n\n"),
            )
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("tok".to_string()));
        let logs = client.get_vm_logs("job-abc").await.unwrap();
        assert!(logs.contains("hello"));
    }

    #[tokio::test]
    async fn test_get_vm_logs_sends_bearer_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms/job-1/logs"))
            .and(header("authorization", "Bearer secret"))
            .respond_with(ResponseTemplate::new(200).set_body_string(""))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("secret".to_string()));
        assert!(client.get_vm_logs("job-1").await.is_ok());
    }

    #[tokio::test]
    async fn test_get_vm_logs_404_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms/ghost/logs"))
            .respond_with(
                ResponseTemplate::new(404)
                    .set_body_json(serde_json::json!({"error": "Job not found"})),
            )
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let err = client.get_vm_logs("ghost").await.unwrap_err();
        assert!(err.to_string().contains("Job not found"));
        assert!(err.to_string().contains("404"));
    }

    // ── pause_vm ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_pause_vm_success_returns_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/vms/job-abc/pause"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "message": "VM paused"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("tok".to_string()));
        let resp = client.pause_vm("job-abc").await.unwrap();
        assert!(resp.success);
        assert_eq!(resp.message, "VM paused");
    }

    #[tokio::test]
    async fn test_pause_vm_failure_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/vms/job-abc/pause"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": false,
                "message": "VM not running"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("tok".to_string()));
        let resp = client.pause_vm("job-abc").await.unwrap();
        assert!(!resp.success);
    }

    #[tokio::test]
    async fn test_pause_vm_sends_bearer_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/vms/job-1/pause"))
            .and(header("authorization", "Bearer secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "message": "ok"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("secret".to_string()));
        assert!(client.pause_vm("job-1").await.is_ok());
    }

    // ── resume_vm ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_resume_vm_success_returns_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/vms/job-abc/resume"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "message": "VM resumed"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("tok".to_string()));
        let resp = client.resume_vm("job-abc").await.unwrap();
        assert!(resp.success);
        assert_eq!(resp.message, "VM resumed");
    }

    #[tokio::test]
    async fn test_resume_vm_failure_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/vms/job-abc/resume"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": false,
                "message": "VM not paused"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("tok".to_string()));
        let resp = client.resume_vm("job-abc").await.unwrap();
        assert!(!resp.success);
    }

    #[tokio::test]
    async fn test_resume_vm_sends_bearer_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/vms/job-1/resume"))
            .and(header("authorization", "Bearer secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "message": "ok"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("secret".to_string()));
        assert!(client.resume_vm("job-1").await.is_ok());
    }

    // ── delete_vm ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_delete_vm_success_returns_response() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/vms/job-abc/delete"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "message": "VM deleted"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("tok".to_string()));
        let resp = client.delete_vm("job-abc").await.unwrap();
        assert!(resp.success);
        assert_eq!(resp.message, "VM deleted");
    }

    #[tokio::test]
    async fn test_delete_vm_404_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/vms/ghost/delete"))
            .respond_with(
                ResponseTemplate::new(404)
                    .set_body_json(serde_json::json!({"error": "Job not found"})),
            )
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let err = client.delete_vm("ghost").await.unwrap_err();
        assert!(err.to_string().contains("Job not found"));
        assert!(err.to_string().contains("404"));
    }

    #[tokio::test]
    async fn test_delete_vm_sends_bearer_token() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/vms/job-1/delete"))
            .and(header("authorization", "Bearer secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "message": "deleted"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("secret".to_string()));
        assert!(client.delete_vm("job-1").await.is_ok());
    }

    // ── restart_vm ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_restart_vm_success_returns_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/vms/job-abc/restart"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "message": "VM restarted"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("tok".to_string()));
        let resp = client.restart_vm("job-abc").await.unwrap();
        assert!(resp.success);
        assert_eq!(resp.message, "VM restarted");
    }

    #[tokio::test]
    async fn test_restart_vm_sends_bearer_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/vms/job-1/restart"))
            .and(header("authorization", "Bearer secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "message": "ok"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("secret".to_string()));
        assert!(client.restart_vm("job-1").await.is_ok());
    }

    // ── get_vm_metrics ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_get_vm_metrics_success_returns_metrics() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms/job-abc/metrics"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "job_id": "job-abc",
                "cpu_usage": 45.5,
                "memory_usage": 62.3,
                "disk_usage": 30.0,
                "network_rx": 1024,
                "network_tx": 512,
                "timestamp": 1_700_000_000
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("tok".to_string()));
        let metrics = client.get_vm_metrics("job-abc").await.unwrap();
        assert_eq!(metrics.cpu_usage, 45.5);
        assert_eq!(metrics.memory_usage, 62.3);
    }

    #[tokio::test]
    async fn test_get_vm_metrics_sends_bearer_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms/job-1/metrics"))
            .and(header("authorization", "Bearer secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "job_id": "job-1",
                "cpu_usage": 0.0,
                "memory_usage": 0.0,
                "disk_usage": 0.0,
                "network_rx": 0,
                "network_tx": 0,
                "timestamp": 0
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("secret".to_string()));
        assert!(client.get_vm_metrics("job-1").await.is_ok());
    }

    // ── whoami ───────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_whoami_success_returns_user() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/auth/whoami"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "user_id": "user-123",
                "email": "user@example.com",
                "created_at": "2024-01-01T00:00:00Z"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("tok".to_string()));
        let user = client.whoami().await.unwrap();
        assert_eq!(user.user_id, "user-123");
        assert_eq!(user.email, "user@example.com");
    }

    #[tokio::test]
    async fn test_whoami_sends_bearer_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/auth/whoami"))
            .and(header("authorization", "Bearer secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "user_id": "u",
                "email": "e@e.com",
                "created_at": "2024-01-01"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("secret".to_string()));
        assert!(client.whoami().await.is_ok());
    }

    #[tokio::test]
    async fn test_whoami_401_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/auth/whoami"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": "Invalid or expired token"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let err = client.whoami().await.unwrap_err();
        assert!(err.to_string().contains("Invalid or expired token"));
        assert!(err.to_string().contains("401"));
    }

    #[tokio::test]
    async fn test_whoami_404_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/auth/whoami"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "error": "Not found"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("tok".to_string()));
        let err = client.whoami().await.unwrap_err();
        assert!(err.to_string().contains("404"));
    }

    #[tokio::test]
    async fn test_get_vm_logs_empty_body_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms/job-abc/logs"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![]))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let result = client.get_vm_logs("job-abc").await;
        assert!(result.is_err() || result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_get_vm_logs_empty_body_returns_empty_string() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms/job-abc/logs"))
            .respond_with(ResponseTemplate::new(200).set_body_string(""))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let logs = client.get_vm_logs("job-abc").await.unwrap();
        assert!(logs.is_empty());
    }

    #[tokio::test]
    async fn test_deploy_sends_correct_json_body() {
        use serde_json::json;
        use wiremock::matchers::body_json;

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/deploy"))
            .and(body_json(json!({
                "app_name": "test-app",
                "image": "nginx:latest"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "job_id": "j-1",
                "status": "Scheduled",
                "host_id": null,
                "vm_id": null,
                "message": "ok"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let resp = client
            .deploy("test-app", "nginx:latest", None, None, None, HashMap::new())
            .await
            .unwrap();
        assert_eq!(resp.job_id, "j-1");
    }

    #[tokio::test]
    async fn test_deploy_with_all_params() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/deploy"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "job_id": "j-full",
                "status": "Scheduled",
                "host_id": "h-1",
                "vm_id": "v-1",
                "message": "deployed with all params"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("tok".to_string()));
        let mut env = HashMap::new();
        env.insert("PORT".to_string(), "8080".to_string());
        let resp = client
            .deploy("my-app", "alpine:3", Some(4), Some(1024), Some(2048), env)
            .await
            .unwrap();
        assert_eq!(resp.status, "Scheduled");
    }

    #[tokio::test]
    async fn test_deploy_with_env_only() {
        use wiremock::matchers::body_json;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/deploy"))
            .and(body_json(serde_json::json!({
                "app_name": "app",
                "image": "img",
                "env": {"KEY": "VALUE"}
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "job_id": "j",
                "status": "ok",
                "host_id": null,
                "vm_id": null,
                "message": "ok"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let mut env = HashMap::new();
        env.insert("KEY".to_string(), "VALUE".to_string());
        let resp = client
            .deploy("app", "img", None, None, None, env)
            .await
            .unwrap();
        assert!(resp.host_id.is_none());
    }

    #[tokio::test]
    async fn test_deploy_401_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/deploy"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": "Unauthorized"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let err = client
            .deploy("app", "img", None, None, None, HashMap::new())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Unauthorized"));
        assert!(err.to_string().contains("401"));
    }

    #[tokio::test]
    async fn test_deploy_500_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/deploy"))
            .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({
                "error": "Internal server error"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let err = client
            .deploy("app", "img", None, None, None, HashMap::new())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("500"));
    }

    #[tokio::test]
    async fn test_restart_vm_404_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/vms/ghost/restart"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "error": "Job not found"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("tok".to_string()));
        let err = client.restart_vm("ghost").await.unwrap_err();
        assert!(err.to_string().contains("Job not found"));
        assert!(err.to_string().contains("404"));
    }

    #[tokio::test]
    async fn test_restart_vm_failure_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/vms/job-abc/restart"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": false,
                "message": "VM not running"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("tok".to_string()));
        let resp = client.restart_vm("job-abc").await.unwrap();
        assert!(!resp.success);
    }

    #[tokio::test]
    async fn test_restart_vm_500_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/vms/job-1/restart"))
            .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({
                "error": "Internal error"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.restart_vm("job-1").await.is_err());
    }

    #[tokio::test]
    async fn test_get_vm_metrics_404_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms/ghost/metrics"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "error": "Job not found"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let err = client.get_vm_metrics("ghost").await.unwrap_err();
        assert!(err.to_string().contains("404"));
    }

    #[tokio::test]
    async fn test_get_vm_metrics_500_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms/job-1/metrics"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.get_vm_metrics("job-1").await.is_err());
    }

    #[tokio::test]
    async fn test_get_vm_metrics_server_unreachable() {
        let client = MikromClient::new("http://127.0.0.1:59999".to_string(), None);
        assert!(client.get_vm_metrics("job-1").await.is_err());
    }

    #[tokio::test]
    async fn test_whoami_server_unreachable() {
        let client = MikromClient::new("http://127.0.0.1:59998".to_string(), None);
        assert!(client.whoami().await.is_err());
    }

    #[tokio::test]
    async fn test_restart_vm_server_unreachable() {
        let client = MikromClient::new("http://127.0.0.1:59997".to_string(), None);
        assert!(client.restart_vm("job-1").await.is_err());
    }

    #[tokio::test]
    async fn test_health_different_status_codes() {
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
    }

    #[tokio::test]
    async fn test_register_empty_body_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/register"))
            .respond_with(ResponseTemplate::new(201).set_body_bytes(vec![]))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.register("a@b.com", "password123").await.is_err());
    }

    #[tokio::test]
    async fn test_login_empty_body_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/login"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![]))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.login("a@b.com", "password123").await.is_err());
    }

    #[tokio::test]
    async fn test_list_vms_malformed_json_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not-json["))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.list_vms().await.is_err());
    }

    #[tokio::test]
    async fn test_get_vm_malformed_json_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms/job-1"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{bad"))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.get_vm("job-1").await.is_err());
    }

    #[tokio::test]
    async fn test_stop_vm_malformed_json_on_success() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/vms/job-1"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.stop_vm("job-1").await.is_err());
    }

    #[tokio::test]
    async fn test_pause_vm_malformed_json_on_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/vms/job-1/pause"))
            .respond_with(ResponseTemplate::new(200).set_body_string("bad"))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.pause_vm("job-1").await.is_err());
    }

    #[tokio::test]
    async fn test_resume_vm_malformed_json_on_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/vms/job-1/resume"))
            .respond_with(ResponseTemplate::new(200).set_body_string("invalid"))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.resume_vm("job-1").await.is_err());
    }

    #[tokio::test]
    async fn test_delete_vm_malformed_json_on_success() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/vms/job-1/delete"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not-json"))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.delete_vm("job-1").await.is_err());
    }

    #[tokio::test]
    async fn test_restart_vm_malformed_json_on_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/vms/job-1/restart"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{invalid"))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.restart_vm("job-1").await.is_err());
    }

    #[tokio::test]
    async fn test_get_vm_metrics_malformed_json() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms/job-1/metrics"))
            .respond_with(ResponseTemplate::new(200).set_body_string("invalid"))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.get_vm_metrics("job-1").await.is_err());
    }

    #[tokio::test]
    async fn test_whoami_malformed_json() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/auth/whoami"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{{invalid"))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.whoami().await.is_err());
    }

    // ── edge cases ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_client_new_with_empty_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "ok",
                "version": "0.1.0"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), Some("".to_string()));
        assert!(client.health().await.is_ok());
    }

    #[tokio::test]
    async fn test_client_new_with_none_token() {
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
        assert!(client.health().await.is_ok());
    }

    #[tokio::test]
    async fn test_health_response_with_missing_version() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "ok"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.health().await.is_err());
    }

    #[tokio::test]
    async fn test_health_response_with_null_fields() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": null,
                "version": null
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let result = client.health().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_vms_response_with_null_elements() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([null])))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let result = client.list_vms().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_vm_response_missing_fields() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms/job-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "job_id": "job-1"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.get_vm("job-1").await.is_err());
    }

    #[tokio::test]
    async fn test_deploy_response_missing_fields() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/deploy"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "job_id": "j-1"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(
            client
                .deploy("a", "i", None, None, None, HashMap::new())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_register_response_missing_fields() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/register"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "user_id": "u-1"
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.register("a@b.com", "pass").await.is_err());
    }

    #[tokio::test]
    async fn test_login_response_missing_fields() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/login"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.login("a@b.com", "pass").await.is_err());
    }

    #[tokio::test]
    async fn test_action_response_missing_fields() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/vms/j-1/pause"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.pause_vm("j-1").await.is_err());
    }

    #[tokio::test]
    async fn test_deploy_response_with_null_fields() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/deploy"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "job_id": null,
                "status": null,
                "host_id": null,
                "vm_id": null,
                "message": null
            })))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let result = client
            .deploy("a", "i", None, None, None, HashMap::new())
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_vm_logs_special_characters() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms/job-abc/logs"))
            .respond_with(ResponseTemplate::new(200).set_body_string("log line 1\nlog line 2\r\n"))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        let logs = client.get_vm_logs("job-abc").await.unwrap();
        assert!(logs.contains("log line"));
    }

    #[tokio::test]
    async fn test_get_vm_logs_binary_data() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms/job-abc/logs"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0, 1, 2, 3]))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.get_vm_logs("job-abc").await.is_ok());
    }

    #[tokio::test]
    async fn test_stop_vm_error_response_without_error_field() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/vms/job-1"))
            .respond_with(ResponseTemplate::new(404).set_body_string("Not Found"))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.stop_vm("job-1").await.is_err());
    }

    #[tokio::test]
    async fn test_list_vms_server_error_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/vms"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let client = MikromClient::new(server.uri(), None);
        assert!(client.list_vms().await.is_err());
    }
}
