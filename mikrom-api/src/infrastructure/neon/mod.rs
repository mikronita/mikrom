use crate::config::ApiConfig;
use crate::domain::{DomainError, DomainResult};
use std::sync::LazyLock;
use uuid::Uuid;

static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent("mikrom-api")
        .build()
        .expect("Failed to create reqwest client")
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeonProvisioning {
    pub tenant_id: String,
    pub timeline_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantLocationMode {
    AttachedSingle,
    AttachedMulti,
    AttachedStale,
    Secondary,
    Detached,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TenantGeneration(u64);

impl TenantGeneration {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn initial() -> Self {
        Self(1)
    }

    fn value(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TenantLocationConfig {
    mode: TenantLocationMode,
    generation: TenantGeneration,
}

impl TenantLocationConfig {
    pub fn attached_single(generation: TenantGeneration) -> Self {
        Self {
            mode: TenantLocationMode::AttachedSingle,
            generation,
        }
    }
}

#[derive(Clone)]
pub struct NeonClient {
    base_url: String,
    bearer_token: Option<String>,
}

impl NeonClient {
    pub fn new(base_url: String, bearer_token: Option<String>) -> Self {
        Self {
            base_url,
            bearer_token,
        }
    }

    pub fn from_config(config: &ApiConfig) -> Option<Self> {
        config
            .neon_pageserver_url
            .as_ref()
            .map(|base_url| Self::new(base_url.clone(), config.neon_bearer_token.clone()))
    }

    pub async fn provision_database(&self) -> DomainResult<NeonProvisioning> {
        let tenant_id = Uuid::new_v4().simple().to_string();
        let timeline_id = Uuid::new_v4().simple().to_string();

        self.provision_database_with_ids(tenant_id, timeline_id)
            .await
    }

    pub async fn provision_database_with_ids(
        &self,
        tenant_id: String,
        timeline_id: String,
    ) -> DomainResult<NeonProvisioning> {
        self.create_tenant(&tenant_id).await?;
        self.attach_tenant(
            &tenant_id,
            TenantLocationConfig::attached_single(TenantGeneration::initial()),
        )
        .await?;
        self.create_timeline(&tenant_id, &timeline_id).await?;

        Ok(NeonProvisioning {
            tenant_id,
            timeline_id,
        })
    }

    async fn create_tenant(&self, tenant_id: &str) -> DomainResult<()> {
        let response = self
            .request(reqwest::Method::POST, "/v1/tenant")
            .json(&serde_json::json!({ "new_tenant_id": tenant_id }))
            .send()
            .await
            .map_err(|e| DomainError::Infrastructure(format!("Neon tenant request failed: {e}")))?;

        self.ensure_success(response, "create tenant").await?;
        Ok(())
    }

    async fn create_timeline(&self, tenant_id: &str, timeline_id: &str) -> DomainResult<()> {
        let response = self
            .request(
                reqwest::Method::POST,
                &format!("/v1/tenant/{tenant_id}/timeline"),
            )
            .json(&serde_json::json!({ "new_timeline_id": timeline_id }))
            .send()
            .await
            .map_err(|e| {
                DomainError::Infrastructure(format!("Neon timeline request failed: {e}"))
            })?;

        self.ensure_success(response, "create timeline").await?;
        Ok(())
    }

    async fn attach_tenant(
        &self,
        tenant_id: &str,
        config: TenantLocationConfig,
    ) -> DomainResult<()> {
        let response = self
            .request(
                reqwest::Method::PUT,
                &format!("/v1/tenant/{tenant_id}/location_config"),
            )
            .json(&serde_json::json!({
                "mode": config.mode.as_str(),
                "generation": config.generation.value(),
            }))
            .send()
            .await
            .map_err(|e| {
                DomainError::Infrastructure(format!("Neon tenant location request failed: {e}"))
            })?;

        self.ensure_success(response, "attach tenant").await?;
        Ok(())
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        let builder = HTTP_CLIENT.request(method, url);
        if let Some(token) = &self.bearer_token {
            builder.bearer_auth(token)
        } else {
            builder
        }
    }

    async fn ensure_success(&self, response: reqwest::Response, action: &str) -> DomainResult<()> {
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }

        let body = response.text().await.unwrap_or_default();
        Err(DomainError::Infrastructure(format!(
            "Neon {action} failed: {status} - {body}"
        )))
    }
}

impl TenantLocationMode {
    fn as_str(self) -> &'static str {
        match self {
            TenantLocationMode::AttachedSingle => "AttachedSingle",
            TenantLocationMode::AttachedMulti => "AttachedMulti",
            TenantLocationMode::AttachedStale => "AttachedStale",
            TenantLocationMode::Secondary => "Secondary",
            TenantLocationMode::Detached => "Detached",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn provision_database_posts_tenant_and_timeline() {
        let server = MockServer::start().await;
        let token = "jwt-token";

        Mock::given(method("POST"))
            .and(path("/v1/tenant"))
            .and(header("authorization", format!("Bearer {token}")))
            .and(body_json(serde_json::json!({
                "new_tenant_id": "11111111111111111111111111111111"
            })))
            .respond_with(
                ResponseTemplate::new(201).set_body_string("\"11111111111111111111111111111111\""),
            )
            .mount(&server)
            .await;

        Mock::given(method("PUT"))
            .and(path(
                "/v1/tenant/11111111111111111111111111111111/location_config",
            ))
            .and(header("authorization", format!("Bearer {token}")))
            .and(body_json(serde_json::json!({
                "mode": "AttachedSingle",
                "generation": 1
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "shards": [],
                "stripe_size": null
            })))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/v1/tenant/11111111111111111111111111111111/timeline"))
            .and(header("authorization", format!("Bearer {token}")))
            .and(body_json(serde_json::json!({
                "new_timeline_id": "22222222222222222222222222222222"
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "timeline_id": "22222222222222222222222222222222",
                "tenant_id": "11111111111111111111111111111111",
                "last_record_lsn": "0/0",
                "disk_consistent_lsn": "0/0",
                "state": "active",
                "min_readable_lsn": "0/0"
            })))
            .mount(&server)
            .await;

        let client = NeonClient::new(server.uri(), Some(token.to_string()));
        let provisioning = client
            .provision_database_with_ids(
                "11111111111111111111111111111111".to_string(),
                "22222222222222222222222222222222".to_string(),
            )
            .await
            .unwrap();

        assert_eq!(provisioning.tenant_id, "11111111111111111111111111111111");
        assert_eq!(provisioning.timeline_id, "22222222222222222222222222222222");
    }

    #[test]
    fn attached_single_location_config_is_explicit() {
        let config = TenantLocationConfig::attached_single(TenantGeneration::new(42));
        assert_eq!(config.mode, TenantLocationMode::AttachedSingle);
        assert_eq!(config.generation, TenantGeneration::new(42));
    }
}
