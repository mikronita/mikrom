use crate::application::ports::ApiClient;
use crate::domain::error::{CliError, CliResult};
use crate::domain::models::*;
use crate::infrastructure::http::error::map_http_error;
use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};

pub struct ReqwestApiClient {
    http: reqwest::Client,
    base_url: String,
    token: Option<String>,
    active_project_slug: Option<String>,
    request_timeout: std::time::Duration,
    delete_timeout: std::time::Duration,
    restore_timeout: std::time::Duration,
    long_timeout: std::time::Duration,
}

#[derive(serde::Deserialize)]
struct ErrorResponse {
    error: String,
}

impl ReqwestApiClient {
    pub fn new(
        base_url: String,
        token: Option<String>,
        active_project_slug: Option<String>,
    ) -> CliResult<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(CliError::Http)?;
        Ok(Self {
            http,
            base_url,
            token,
            active_project_slug,
            request_timeout: std::time::Duration::from_secs(30),
            delete_timeout: std::time::Duration::from_secs(120),
            restore_timeout: std::time::Duration::from_secs(60),
            long_timeout: std::time::Duration::from_secs(30),
        })
    }

    pub fn with_timeouts(
        mut self,
        request_timeout: std::time::Duration,
        delete_timeout: std::time::Duration,
        restore_timeout: std::time::Duration,
        long_timeout: std::time::Duration,
    ) -> Self {
        self.request_timeout = request_timeout;
        self.delete_timeout = delete_timeout;
        self.restore_timeout = restore_timeout;
        self.long_timeout = long_timeout;
        self
    }

    async fn request<T: DeserializeOwned, B: Serialize>(
        &self,
        method: reqwest::Method,
        endpoint: &str,
        body: Option<B>,
    ) -> CliResult<T> {
        self.request_with_timeout(method, endpoint, body, self.request_timeout)
            .await
    }

    /// Decide whether a response status warrants a retry.
    fn is_retryable(status: reqwest::StatusCode) -> bool {
        status.is_server_error()
            || status == reqwest::StatusCode::TOO_MANY_REQUESTS
            || status == reqwest::StatusCode::REQUEST_TIMEOUT
    }

    async fn execute_with_retry<F, Fut>(&self, operation: F) -> CliResult<reqwest::Response>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<reqwest::Response, reqwest::Error>>,
    {
        let max_retries = 3;

        for attempt in 0..max_retries {
            match operation().await {
                Ok(resp) => {
                    if resp.status().is_success()
                        || !Self::is_retryable(resp.status())
                        || attempt == max_retries - 1
                    {
                        return Ok(resp);
                    }
                    // Server error: wait and retry
                    let delay = std::time::Duration::from_millis(200 * (1 << attempt));
                    tracing::warn!(
                        status = %resp.status(),
                        attempt = attempt + 1,
                        "Retryable error, waiting {:?} before retry",
                        delay
                    );
                    tokio::time::sleep(delay).await;
                },
                Err(e) => {
                    if (e.is_timeout() || e.is_connect()) && attempt < max_retries - 1 {
                        let delay = std::time::Duration::from_millis(200 * (1 << attempt));
                        tracing::warn!(
                            error = %e,
                            attempt = attempt + 1,
                            "Network error, waiting {:?} before retry",
                            delay
                        );
                        tokio::time::sleep(delay).await;
                    } else {
                        return Err(CliError::Http(e));
                    }
                },
            }
        }

        unreachable!()
    }

    fn build_request<B: Serialize>(
        &self,
        method: reqwest::Method,
        endpoint: &str,
        body: Option<B>,
    ) -> reqwest::RequestBuilder {
        let url = format!(
            "{}/v1/{}",
            self.base_url.trim_end_matches('/'),
            endpoint.trim_start_matches('/')
        );
        let mut builder = self.http.request(method, url);

        if let Some(token) = &self.token {
            builder = builder.bearer_auth(token);
        }

        if let Some(project_slug) = &self.active_project_slug {
            builder = builder.header("x-mikrom-tenant-id", project_slug);
        }

        if let Some(body) = body {
            builder = builder.json(&body);
        }

        builder
    }

    async fn request_with_timeout<T: DeserializeOwned, B: Serialize>(
        &self,
        method: reqwest::Method,
        endpoint: &str,
        body: Option<B>,
        timeout: std::time::Duration,
    ) -> CliResult<T> {
        let builder = self.build_request(method, endpoint, body);
        let resp = self
            .execute_with_retry(|| async {
                builder.try_clone().unwrap().timeout(timeout).send().await
            })
            .await?;

        if resp.status().is_success() {
            Ok(resp.json().await.map_err(CliError::Http)?)
        } else {
            let status = resp.status();
            let err_body: ErrorResponse = resp.json().await.map_err(|_| CliError::Api {
                status: status.as_u16(),
                message: "Failed to parse error response".to_string(),
            })?;
            Err(map_http_error(status, err_body.error))
        }
    }

    async fn request_no_content_with_timeout<B: Serialize>(
        &self,
        method: reqwest::Method,
        endpoint: &str,
        body: Option<B>,
        timeout: std::time::Duration,
    ) -> CliResult<()> {
        let builder = self.build_request(method, endpoint, body);
        let resp = self
            .execute_with_retry(|| async {
                builder.try_clone().unwrap().timeout(timeout).send().await
            })
            .await?;

        if resp.status().is_success() {
            return Ok(());
        }

        let status = resp.status();
        let err_body: ErrorResponse = resp.json().await.map_err(|_| CliError::Api {
            status: status.as_u16(),
            message: "Failed to parse error response".to_string(),
        })?;
        Err(map_http_error(status, err_body.error))
    }

    async fn request_no_body(
        &self,
        method: reqwest::Method,
        endpoint: &str,
        timeout: std::time::Duration,
    ) -> CliResult<()> {
        self.request_no_content_with_timeout(method, endpoint, None::<()>, timeout)
            .await
    }
}

#[async_trait]
impl ApiClient for ReqwestApiClient {
    async fn health(&self) -> CliResult<HealthResponse> {
        self.request(reqwest::Method::GET, "health", None::<()>)
            .await
    }

    async fn register(&self, email: &str, password: &str) -> CliResult<RegisterResponse> {
        self.request(
            reqwest::Method::POST,
            "/auth/register",
            Some(serde_json::json!({ "email": email, "password": password })),
        )
        .await
    }

    async fn login(&self, email: &str, password: &str) -> CliResult<LoginResponse> {
        self.request(
            reqwest::Method::POST,
            "/auth/login",
            Some(serde_json::json!({ "email": email, "password": password })),
        )
        .await
    }

    async fn whoami(&self) -> CliResult<WhoamiResponse> {
        self.request(reqwest::Method::GET, "/auth/me", None::<()>)
            .await
    }

    async fn update_profile(
        &self,
        first_name: Option<String>,
        last_name: Option<String>,
    ) -> CliResult<WhoamiResponse> {
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

    async fn list_apps(&self) -> CliResult<Vec<AppInfo>> {
        self.request(reqwest::Method::GET, "/apps", None::<()>)
            .await
    }

    async fn get_app(&self, app_name: &str) -> CliResult<AppInfo> {
        self.request(
            reqwest::Method::GET,
            &format!("/apps/{}", app_name),
            None::<()>,
        )
        .await
    }

    async fn create_app(&self, name: &str, git_url: &str) -> CliResult<AppInfo> {
        self.request(
            reqwest::Method::POST,
            "/apps",
            Some(serde_json::json!({ "name": name, "git_url": git_url })),
        )
        .await
    }

    async fn delete_app(&self, app_id: &str) -> CliResult<()> {
        self.request_no_content_with_timeout(
            reqwest::Method::DELETE,
            &format!("/apps/{}", app_id),
            None::<()>,
            self.delete_timeout,
        )
        .await
    }

    async fn get_app_secret(&self, app_name: &str) -> CliResult<Option<String>> {
        let body: serde_json::Value = self
            .request(
                reqwest::Method::GET,
                &format!("/apps/{}/secret", app_name),
                None::<()>,
            )
            .await?;
        Ok(body["github_webhook_secret"]
            .as_str()
            .map(|s| s.to_string()))
    }

    async fn deploy_app_version(
        &self,
        app_id: &str,
        vcpus: u32,
        memory_mib: u32,
        hypervisor: Option<String>,
    ) -> CliResult<DeployResponse> {
        let mut body = serde_json::json!({
            "vcpus": vcpus,
            "memory_mib": memory_mib,
        });
        if let Some(hv) = hypervisor {
            body["hypervisor"] = serde_json::json!(hv);
        }
        self.request(
            reqwest::Method::POST,
            &format!("/apps/{}/deploy", app_id),
            Some(body),
        )
        .await
    }

    async fn activate_deployment(&self, app_id: &str, deployment_id: &str) -> CliResult<()> {
        self.request(
            reqwest::Method::POST,
            &format!("/apps/{}/deployments/{}/activate", app_id, deployment_id),
            None::<()>,
        )
        .await
    }

    async fn list_app_deployments(&self, app_id: &str) -> CliResult<Vec<DeploymentInfo>> {
        self.request(
            reqwest::Method::GET,
            &format!("/apps/{}/deployments", app_id),
            None::<()>,
        )
        .await
    }

    async fn list_active_deployments(&self) -> CliResult<Vec<LiveDeploymentInfo>> {
        self.request(reqwest::Method::GET, "/deployments/active", None::<()>)
            .await
    }

    async fn get_deployment_status(
        &self,
        app_name: &str,
        job_id: &str,
    ) -> CliResult<LiveDeploymentStatus> {
        self.request(
            reqwest::Method::GET,
            &format!("/apps/{}/deployments/{}", app_name, job_id),
            None::<()>,
        )
        .await
    }

    async fn stop_deployment(&self, app_name: &str, job_id: &str) -> CliResult<serde_json::Value> {
        self.request(
            reqwest::Method::DELETE,
            &format!("/apps/{}/deployments/{}", app_name, job_id),
            None::<()>,
        )
        .await
    }

    async fn pause_deployment(&self, app_name: &str, job_id: &str) -> CliResult<serde_json::Value> {
        self.request(
            reqwest::Method::POST,
            &format!("/apps/{}/deployments/{}/pause", app_name, job_id),
            None::<()>,
        )
        .await
    }

    async fn resume_deployment(
        &self,
        app_name: &str,
        job_id: &str,
    ) -> CliResult<serde_json::Value> {
        self.request(
            reqwest::Method::POST,
            &format!("/apps/{}/deployments/{}/resume", app_name, job_id),
            None::<()>,
        )
        .await
    }

    async fn delete_deployment_record(
        &self,
        app_name: &str,
        job_id: &str,
    ) -> CliResult<serde_json::Value> {
        self.request(
            reqwest::Method::DELETE,
            &format!("/apps/{}/deployments/{}/delete", app_name, job_id),
            None::<()>,
        )
        .await
    }

    async fn scale_app(&self, app_id: &str, req: ScaleRequest) -> CliResult<()> {
        self.request_no_content_with_timeout(
            reqwest::Method::PATCH,
            &format!("/apps/{}/scale", app_id),
            Some(req),
            self.long_timeout,
        )
        .await
    }

    async fn list_volumes(&self, app_id: &str) -> CliResult<Vec<AttachedVolume>> {
        self.request(
            reqwest::Method::GET,
            &format!("apps/{}/volumes", app_id),
            None::<()>,
        )
        .await
    }

    async fn list_all_volumes(&self) -> CliResult<Vec<VolumeWithAttachments>> {
        self.request(reqwest::Method::GET, "volumes", None::<()>)
            .await
    }

    async fn create_volume(&self, name: &str, size_mib: i32) -> CliResult<Volume> {
        let body = serde_json::json!({
            "name": name,
            "size_mib": size_mib,
        });
        self.request(reqwest::Method::POST, "volumes", Some(body))
            .await
    }

    async fn attach_volume(
        &self,
        app_id: &str,
        volume_id: &str,
        mount_point: &str,
        access_mode: i32,
    ) -> CliResult<AppVolume> {
        let body = serde_json::json!({
            "volume_id": volume_id,
            "mount_point": mount_point,
            "access_mode": access_mode,
        });
        self.request(
            reqwest::Method::POST,
            &format!("apps/{}/volumes/attach", app_id),
            Some(body),
        )
        .await
    }

    async fn detach_volume(&self, app_id: &str, volume_id: &str) -> CliResult<()> {
        self.request_no_body(
            reqwest::Method::DELETE,
            &format!("apps/{}/volumes/{}/detach", app_id, volume_id),
            self.delete_timeout,
        )
        .await
    }

    async fn create_volume_snapshot(
        &self,
        volume_id: &str,
        name: &str,
    ) -> CliResult<VolumeSnapshot> {
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

    async fn list_volume_snapshots(&self, volume_id: &str) -> CliResult<Vec<VolumeSnapshot>> {
        self.request(
            reqwest::Method::GET,
            &format!("volumes/{}/snapshots", volume_id),
            None::<()>,
        )
        .await
    }

    async fn restore_volume_snapshot(&self, volume_id: &str, snapshot_name: &str) -> CliResult<()> {
        let body = serde_json::json!({
            "snapshot_name": snapshot_name
        });
        self.request_no_content_with_timeout(
            reqwest::Method::POST,
            &format!("volumes/{}/restore", volume_id),
            Some(body),
            self.restore_timeout,
        )
        .await
    }

    async fn delete_volume_snapshot(&self, snapshot_id: &str) -> CliResult<()> {
        self.request_no_body(
            reqwest::Method::DELETE,
            &format!("snapshots/{}", snapshot_id),
            self.delete_timeout,
        )
        .await
    }

    async fn delete_volume(&self, volume_id: &str) -> CliResult<()> {
        self.request_no_body(
            reqwest::Method::DELETE,
            &format!("volumes/{}", volume_id),
            self.delete_timeout,
        )
        .await
    }

    async fn list_databases(&self) -> CliResult<Vec<DatabaseInfo>> {
        self.request::<Vec<DatabaseInfo>, ()>(reqwest::Method::GET, "databases", None)
            .await
    }

    async fn create_database(&self, req: CreateDatabaseRequest) -> CliResult<DatabaseInfo> {
        self.request(reqwest::Method::POST, "databases", Some(req))
            .await
    }

    async fn delete_database(&self, db_id: &str) -> CliResult<()> {
        self.request_no_body(
            reqwest::Method::DELETE,
            &format!("databases/{}", db_id),
            self.delete_timeout,
        )
        .await
    }

    async fn get_database_connection_info(&self, db_id: &str) -> CliResult<DatabaseConnectionInfo> {
        self.request(
            reqwest::Method::GET,
            &format!("databases/{}/connection", db_id),
            None::<()>,
        )
        .await
    }

    async fn list_projects(&self) -> CliResult<Vec<ProjectInfo>> {
        self.request(reqwest::Method::GET, "projects", None::<()>)
            .await
    }

    async fn create_project(&self, name: &str) -> CliResult<ProjectInfo> {
        self.request(
            reqwest::Method::POST,
            "projects",
            Some(serde_json::json!({ "name": name })),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    #[tokio::test]
    async fn delete_database_uses_delete_timeout_instead_of_request_timeout() {
        let server = MockServer::start().await;
        let client = ReqwestApiClient::new(server.uri(), None, None)
            .expect("client should build")
            .with_timeouts(
                std::time::Duration::from_millis(25),
                std::time::Duration::from_millis(250),
                std::time::Duration::from_millis(100),
                std::time::Duration::from_millis(100),
            );

        Mock::given(method("DELETE"))
            .and(path("/v1/databases/db-123"))
            .respond_with(
                ResponseTemplate::new(204).set_delay(std::time::Duration::from_millis(75)),
            )
            .expect(1)
            .mount(&server)
            .await;

        client
            .delete_database("db-123")
            .await
            .expect("delete_database should use the longer delete timeout");
    }
}
