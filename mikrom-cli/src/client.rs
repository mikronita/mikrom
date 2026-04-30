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
    pub services: HashMap<String, String>,
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
    pub job_id: Option<String>,
    pub deployment_id: Option<String>,
    pub status: String,
    pub host_id: Option<String>,
    pub vm_id: Option<String>,
    pub image_tag: Option<String>,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct AppInfo {
    pub id: String,
    pub name: String,
    pub git_url: String,
    pub port: i32,
    pub hostname: Option<String>,
    pub github_webhook_secret: Option<String>,
    pub active_deployment_id: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct LiveDeploymentInfo {
    pub job_id: String,
    pub deployment_id: String,
    pub app_id: String,
    pub app_name: String,
    pub image: String,
    pub status: String,
    pub host_id: String,
    pub vm_id: String,
}

#[derive(Debug, Deserialize)]
pub struct LiveDeploymentStatus {
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

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct DeploymentInfo {
    pub id: String,
    pub app_id: String,
    pub build_id: Option<String>,
    pub image_tag: Option<String>,
    pub job_id: Option<String>,
    pub status: String,
    pub created_at: Option<String>,
    pub updated_at: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct VmMetrics {
    pub cpu_usage: f32,
    pub memory_usage: f32,
    pub disk_usage: f32,
    pub network_rx: u64,
    pub network_tx: u64,
}
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct VmMetricsResponse {
    pub job_id: String,
    pub metrics: VmMetrics,
}

#[derive(Debug, Deserialize)]
pub struct WhoamiResponse {
    #[serde(alias = "id")]
    pub user_id: String,
    pub email: String,
    pub role: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub created_at: Option<String>,
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

        let resp = req.send().await?;
        if resp.status().is_success() {
            Ok(resp.json().await?)
        } else {
            let status = resp.status().as_u16();
            let err: ErrorResponse = resp.json().await?;
            bail!("{} (HTTP {})", err.error, status);
        }
    }

    pub async fn list_active_deployments(&self) -> anyhow::Result<Vec<LiveDeploymentInfo>> {
        let mut req = self
            .http
            .get(format!("{}/deployments/active", self.base_url));
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

    pub async fn get_deployment_status(
        &self,
        app_name: &str,
        job_id: &str,
    ) -> anyhow::Result<LiveDeploymentStatus> {
        let mut req = self.http.get(format!(
            "{}/apps/{}/deployments/{}",
            self.base_url, app_name, job_id
        ));
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

    pub async fn stop_deployment(
        &self,
        app_name: &str,
        job_id: &str,
    ) -> anyhow::Result<serde_json::Value> {
        let mut req = self.http.delete(format!(
            "{}/apps/{}/deployments/{}",
            self.base_url, app_name, job_id
        ));
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

    pub async fn delete_deployment_record(
        &self,
        app_name: &str,
        job_id: &str,
    ) -> anyhow::Result<serde_json::Value> {
        let mut req = self.http.delete(format!(
            "{}/apps/{}/deployments/{}/delete",
            self.base_url, app_name, job_id
        ));
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

    pub async fn pause_deployment(
        &self,
        app_name: &str,
        job_id: &str,
    ) -> anyhow::Result<serde_json::Value> {
        let mut req = self.http.post(format!(
            "{}/apps/{}/deployments/{}/pause",
            self.base_url, app_name, job_id
        ));
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

    pub async fn resume_deployment(
        &self,
        app_name: &str,
        job_id: &str,
    ) -> anyhow::Result<serde_json::Value> {
        let mut req = self.http.post(format!(
            "{}/apps/{}/deployments/{}/resume",
            self.base_url, app_name, job_id
        ));
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

    // ── Apps ──────────────────────────────────────────────────────────────────

    pub async fn create_app(&self, name: &str, git_url: &str) -> anyhow::Result<AppInfo> {
        let mut req = self
            .http
            .post(format!("{}/apps", self.base_url))
            .json(&serde_json::json!({ "name": name, "git_url": git_url }));
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

    pub async fn list_apps(&self) -> anyhow::Result<Vec<AppInfo>> {
        let mut req = self.http.get(format!("{}/apps", self.base_url));
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

    pub async fn delete_app(&self, app_id: &str) -> anyhow::Result<()> {
        let mut req = self
            .http
            .delete(format!("{}/apps/{}", self.base_url, app_id));
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status().as_u16();
            let err: ErrorResponse = resp.json().await?;
            bail!("{} (HTTP {})", err.error, status);
        }
    }

    pub async fn get_app_secret(&self, app_name: &str) -> anyhow::Result<String> {
        let mut req = self
            .http
            .get(format!("{}/apps/{}/secret", self.base_url, app_name));
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await?;
        if resp.status().is_success() {
            let body: serde_json::Value = resp.json().await?;
            Ok(body["github_webhook_secret"]
                .as_str()
                .unwrap_or("")
                .to_string())
        } else {
            let status = resp.status().as_u16();
            let err: ErrorResponse = resp.json().await?;
            bail!("{} (HTTP {})", err.error, status);
        }
    }

    pub async fn deploy_app_version(&self, app_id: &str) -> anyhow::Result<DeployResponse> {
        let mut req = self
            .http
            .post(format!("{}/apps/{}/deploy", self.base_url, app_id))
            .json(&serde_json::json!({}));
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

    pub async fn activate_deployment(
        &self,
        app_id: &str,
        deployment_id: &str,
    ) -> anyhow::Result<()> {
        let mut req = self.http.post(format!(
            "{}/apps/{}/deployments/{}/activate",
            self.base_url, app_id, deployment_id
        ));
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status().as_u16();
            let err: ErrorResponse = resp.json().await?;
            bail!("{} (HTTP {})", err.error, status);
        }
    }

    #[allow(dead_code)]
    pub async fn list_app_deployments(&self, app_id: &str) -> anyhow::Result<Vec<DeploymentInfo>> {
        let mut req = self
            .http
            .get(format!("{}/apps/{}/deployments", self.base_url, app_id));
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

    #[allow(dead_code)]
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
        let mut req = self.http.get(format!("{}/auth/me", self.base_url));
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

        let mut req = self
            .http
            .put(format!("{}/auth/me", self.base_url))
            .json(&body);
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
