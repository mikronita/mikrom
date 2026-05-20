use anyhow::bail;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::HashMap;

pub struct MikromClient {
    http: reqwest::Client,
    base_url: String,
    token: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub services: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RegisterResponse {
    pub message: String,
    pub user_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LoginResponse {
    pub token: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DeployResponse {
    pub job_id: Option<String>,
    pub deployment_id: Option<String>,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AppInfo {
    pub id: String,
    pub name: String,
    pub git_url: String,
    pub port: i32,
    pub hostname: Option<String>,
    pub active_deployment_id: Option<String>,
    #[serde(default)]
    pub desired_replicas: i32,
    #[serde(default)]
    pub min_replicas: i32,
    #[serde(default)]
    pub max_replicas: i32,
    #[serde(default)]
    pub autoscaling_enabled: bool,
    #[serde(default)]
    pub cpu_threshold: f64,
    #[serde(default)]
    pub mem_threshold: f64,
    pub created_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LiveDeploymentInfo {
    pub job_id: String,
    pub app_name: String,
    pub image: String,
    pub status: String,
    pub host_id: String,
    pub ipv6_address: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LiveDeploymentStatus {
    pub job_id: String,
    pub status: String,
    pub host_id: String,
    pub vm_id: String,
    pub scheduled_at: i64,
    pub started_at: i64,
    pub error_message: String,
    pub ipv6_address: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DeploymentInfo {
    pub id: String,
    pub image_tag: Option<String>,
    pub status: String,
    pub created_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WhoamiResponse {
    #[serde(alias = "id")]
    pub user_id: String,
    pub email: String,
    pub role: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Volume {
    pub id: String,
    pub app_id: String,
    pub name: String,
    pub size_mib: i32,
    pub pool_name: String,
    pub mount_point: String,
    pub access_mode: i32,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct ScaleRequest {
    pub desired_replicas: Option<i32>,
    pub min_replicas: Option<i32>,
    pub max_replicas: Option<i32>,
    pub autoscaling_enabled: Option<bool>,
    pub cpu_threshold: Option<f64>,
    pub mem_threshold: Option<f64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct VolumeSnapshot {
    pub id: String,
    pub volume_id: String,
    pub name: String,
    pub created_at: String,
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

    /// Generic helper to execute an HTTP request and handle errors.
    async fn request<T: DeserializeOwned, B: Serialize>(
        &self,
        method: reqwest::Method,
        endpoint: &str,
        body: Option<B>,
    ) -> anyhow::Result<T> {
        self.request_with_timeout(method, endpoint, body, std::time::Duration::from_secs(30))
            .await
    }

    async fn request_with_timeout<T: DeserializeOwned, B: Serialize>(
        &self,
        method: reqwest::Method,
        endpoint: &str,
        body: Option<B>,
        timeout: std::time::Duration,
    ) -> anyhow::Result<T> {
        let url = format!(
            "{}/v1/{}",
            self.base_url.trim_end_matches('/'),
            endpoint.trim_start_matches('/')
        );
        let mut builder = self.http.request(method, url);

        if let Some(token) = &self.token {
            builder = builder.bearer_auth(token);
        }

        if let Some(body) = body {
            builder = builder.json(&body);
        }

        let resp = builder.timeout(timeout).send().await?;

        if resp.status().is_success() {
            Ok(resp.json().await?)
        } else {
            let status = resp.status().as_u16();
            let err_body: ErrorResponse = resp.json().await.map_err(|e| {
                anyhow::anyhow!("Failed to parse error response (HTTP {}): {}", status, e)
            })?;
            bail!("{} (HTTP {})", err_body.error, status);
        }
    }

    async fn request_no_content_with_timeout<B: Serialize>(
        &self,
        method: reqwest::Method,
        endpoint: &str,
        body: Option<B>,
        timeout: std::time::Duration,
    ) -> anyhow::Result<()> {
        let url = format!(
            "{}/v1/{}",
            self.base_url.trim_end_matches('/'),
            endpoint.trim_start_matches('/')
        );
        let mut builder = self.http.request(method, url);

        if let Some(token) = &self.token {
            builder = builder.bearer_auth(token);
        }

        if let Some(body) = body {
            builder = builder.json(&body);
        }

        let resp = builder.timeout(timeout).send().await?;

        if resp.status().is_success() {
            return Ok(());
        }

        let status = resp.status().as_u16();
        let err_body: ErrorResponse = resp.json().await.map_err(|e| {
            anyhow::anyhow!("Failed to parse error response (HTTP {}): {}", status, e)
        })?;
        bail!("{} (HTTP {})", err_body.error, status);
    }

    pub async fn list_volumes(&self, app_id: &str) -> anyhow::Result<Vec<Volume>> {
        self.request(
            reqwest::Method::GET,
            &format!("apps/{}/volumes", app_id),
            None::<()>,
        )
        .await
    }

    pub async fn create_volume(
        &self,
        app_id: &str,
        name: &str,
        size_mib: i32,
        mount_point: &str,
        access_mode: i32,
    ) -> anyhow::Result<Volume> {
        let body = serde_json::json!({
            "name": name,
            "size_mib": size_mib,
            "mount_point": mount_point,
            "access_mode": access_mode,
        });
        self.request(
            reqwest::Method::POST,
            &format!("apps/{}/volumes", app_id),
            Some(body),
        )
        .await
    }

    pub async fn scale_app(&self, app_id: &str, req: ScaleRequest) -> anyhow::Result<()> {
        self.request_no_content_with_timeout(
            reqwest::Method::PATCH,
            &format!("apps/{}/scale", app_id),
            Some(req),
            std::time::Duration::from_secs(30),
        )
        .await
    }

    pub async fn create_volume_snapshot(
        &self,
        volume_id: &str,
        name: &str,
    ) -> anyhow::Result<VolumeSnapshot> {
        let body = serde_json::json!({
            "name": name
        });
        self.request(
            reqwest::Method::POST,
            &format!("volumes/{}/snapshots", volume_id),
            Some(body),
        )
        .await
    }

    pub async fn restore_volume_snapshot(
        &self,
        volume_id: &str,
        snapshot_name: &str,
    ) -> anyhow::Result<()> {
        let body = serde_json::json!({
            "snapshot_name": snapshot_name
        });
        self.request_no_content_with_timeout(
            reqwest::Method::POST,
            &format!("volumes/{}/restore", volume_id),
            Some(body),
            std::time::Duration::from_secs(60),
        )
        .await
    }

    pub async fn delete_volume(&self, volume_id: &str) -> anyhow::Result<()> {
        let url = format!(
            "{}/v1/volumes/{}",
            self.base_url.trim_end_matches('/'),
            volume_id
        );
        let mut builder = self.http.request(reqwest::Method::DELETE, url);
        if let Some(token) = &self.token {
            builder = builder.bearer_auth(token);
        }
        let resp = builder.send().await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status().as_u16();
            let err_body: ErrorResponse = resp.json().await.map_err(|e| {
                anyhow::anyhow!("Failed to parse error response (HTTP {}): {}", status, e)
            })?;
            bail!("{} (HTTP {})", err_body.error, status);
        }
    }

    pub async fn health(&self) -> anyhow::Result<HealthResponse> {
        self.request(reqwest::Method::GET, "health", None::<()>)
            .await
    }

    pub async fn register(&self, email: &str, password: &str) -> anyhow::Result<RegisterResponse> {
        self.request(
            reqwest::Method::POST,
            "/auth/register",
            Some(serde_json::json!({ "email": email, "password": password })),
        )
        .await
    }

    pub async fn login(&self, email: &str, password: &str) -> anyhow::Result<LoginResponse> {
        self.request(
            reqwest::Method::POST,
            "/auth/login",
            Some(serde_json::json!({ "email": email, "password": password })),
        )
        .await
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

        self.request(reqwest::Method::POST, "/deploy", Some(body))
            .await
    }

    pub async fn list_active_deployments(&self) -> anyhow::Result<Vec<LiveDeploymentInfo>> {
        self.request(reqwest::Method::GET, "/deployments/active", None::<()>)
            .await
    }

    pub async fn get_deployment_status(
        &self,
        app_name: &str,
        job_id: &str,
    ) -> anyhow::Result<LiveDeploymentStatus> {
        self.request(
            reqwest::Method::GET,
            &format!("/apps/{}/deployments/{}", app_name, job_id),
            None::<()>,
        )
        .await
    }

    pub async fn stop_deployment(
        &self,
        app_name: &str,
        job_id: &str,
    ) -> anyhow::Result<serde_json::Value> {
        self.request(
            reqwest::Method::DELETE,
            &format!("/apps/{}/deployments/{}", app_name, job_id),
            None::<()>,
        )
        .await
    }

    pub async fn delete_deployment_record(
        &self,
        app_name: &str,
        job_id: &str,
    ) -> anyhow::Result<serde_json::Value> {
        self.request(
            reqwest::Method::DELETE,
            &format!("/apps/{}/deployments/{}/delete", app_name, job_id),
            None::<()>,
        )
        .await
    }

    pub async fn pause_deployment(
        &self,
        app_name: &str,
        job_id: &str,
    ) -> anyhow::Result<serde_json::Value> {
        self.request(
            reqwest::Method::POST,
            &format!("/apps/{}/deployments/{}/pause", app_name, job_id),
            None::<()>,
        )
        .await
    }

    pub async fn resume_deployment(
        &self,
        app_name: &str,
        job_id: &str,
    ) -> anyhow::Result<serde_json::Value> {
        self.request(
            reqwest::Method::POST,
            &format!("/apps/{}/deployments/{}/resume", app_name, job_id),
            None::<()>,
        )
        .await
    }

    // ── Apps ──────────────────────────────────────────────────────────────────

    pub async fn create_app(&self, name: &str, git_url: &str) -> anyhow::Result<AppInfo> {
        self.request(
            reqwest::Method::POST,
            "/apps",
            Some(serde_json::json!({ "name": name, "git_url": git_url })),
        )
        .await
    }

    pub async fn list_apps(&self) -> anyhow::Result<Vec<AppInfo>> {
        self.request(reqwest::Method::GET, "/apps", None::<()>)
            .await
    }

    pub async fn delete_app(&self, app_id: &str) -> anyhow::Result<()> {
        self.request_no_content_with_timeout(
            reqwest::Method::DELETE,
            &format!("/apps/{}", app_id),
            None::<()>,
            std::time::Duration::from_secs(120),
        )
        .await
    }

    pub async fn get_app_secret(&self, app_name: &str) -> anyhow::Result<String> {
        let body: serde_json::Value = self
            .request(
                reqwest::Method::GET,
                &format!("/apps/{}/secret", app_name),
                None::<()>,
            )
            .await?;
        Ok(body["github_webhook_secret"]
            .as_str()
            .unwrap_or("")
            .to_string())
    }

    pub async fn deploy_app_version(
        &self,
        app_id: &str,
        vcpus: u32,
        memory_mib: u32,
    ) -> anyhow::Result<DeployResponse> {
        self.request(
            reqwest::Method::POST,
            &format!("/apps/{}/deploy", app_id),
            Some(serde_json::json!({
                "vcpus": vcpus,
                "memory_mib": memory_mib,
            })),
        )
        .await
    }

    pub async fn activate_deployment(
        &self,
        app_id: &str,
        deployment_id: &str,
    ) -> anyhow::Result<()> {
        self.request(
            reqwest::Method::POST,
            &format!("/apps/{}/deployments/{}/activate", app_id, deployment_id),
            None::<()>,
        )
        .await
    }

    pub async fn list_app_deployments(&self, app_id: &str) -> anyhow::Result<Vec<DeploymentInfo>> {
        self.request(
            reqwest::Method::GET,
            &format!("/apps/{}/deployments", app_id),
            None::<()>,
        )
        .await
    }

    pub async fn whoami(&self) -> anyhow::Result<WhoamiResponse> {
        self.request(reqwest::Method::GET, "/auth/me", None::<()>)
            .await
    }

    pub async fn update_profile(
        &self,
        first_name: Option<String>,
        last_name: Option<String>,
    ) -> anyhow::Result<WhoamiResponse> {
        let mut body = serde_json::json!({});
        if let Some(f) = first_name {
            body["first_name"] = serde_json::json!(f);
        }
        if let Some(l) = last_name {
            body["last_name"] = serde_json::json!(l);
        }

        self.request(reqwest::Method::PUT, "/auth/me", Some(body))
            .await
    }
}
