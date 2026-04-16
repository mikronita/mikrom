use anyhow::bail;
use serde::Deserialize;
use std::collections::HashMap;

pub struct MikromClient {
    http: reqwest::Client,
    base_url: String,
    token: Option<String>,
}

#[derive(Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

#[derive(Deserialize)]
pub struct RegisterResponse {
    pub message: String,
    pub user_id: String,
}

#[derive(Deserialize)]
pub struct LoginResponse {
    pub token: String,
}

#[derive(Deserialize)]
pub struct DeployResponse {
    pub job_id: String,
    pub status: String,
    pub host_id: Option<String>,
    pub vm_id: Option<String>,
    pub message: String,
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
}
