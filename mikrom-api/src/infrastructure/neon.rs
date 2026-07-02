use crate::config::ApiConfig;
use crate::domain::{DomainError, DomainResult};
use std::sync::LazyLock;
use uuid::Uuid;

static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent("mikrom-api")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("Failed to create reqwest client")
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeonProvisioning {
    pub tenant_id: String,
    pub timeline_id: String,
    pub tenant_gen: u32,
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

#[derive(Debug, Clone)]
pub struct NeonClient {
    pageserver_base_url: String,
    safekeeper_http_url: String,
    safekeeper_host_alias: String,
    safekeeper_pg_port: u16,
    pageserver_bearer_token: Option<String>,
    safekeeper_bearer_token: String,
}

impl NeonClient {
    pub fn new(
        pageserver_base_url: String,
        safekeeper_http_url: String,
        safekeeper_host_alias: String,
        safekeeper_pg_port: u16,
        pageserver_bearer_token: Option<String>,
        safekeeper_bearer_token: String,
    ) -> Self {
        Self {
            pageserver_base_url,
            safekeeper_http_url,
            safekeeper_host_alias,
            safekeeper_pg_port,
            pageserver_bearer_token,
            safekeeper_bearer_token,
        }
    }

    pub fn from_config(config: &ApiConfig) -> DomainResult<Option<Self>> {
        let Some(pageserver_base_url) = config
            .neon_pageserver_url
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        else {
            return Ok(None);
        };

        let safekeeper_http_url = config
            .neon_safekeeper_http_url
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                DomainError::Infrastructure(
                    "NEON_SAFEKEEPER_HTTP_URL is required to provision Neon databases".to_string(),
                )
            })?
            .clone();

        let safekeeper_connstr = config
            .neon_safekeeper_connstrs
            .as_ref()
            .and_then(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .find(|entry| !entry.is_empty())
            })
            .ok_or_else(|| {
                DomainError::Infrastructure(
                    "NEON_SAFEKEEPER_CONNSTRS is required to provision Neon databases".to_string(),
                )
            })?;
        let (safekeeper_host_alias, safekeeper_pg_port) =
            Self::parse_safekeeper_connstr(safekeeper_connstr)?;
        let safekeeper_bearer_token = config
            .neon_safekeeper_token
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                DomainError::Infrastructure(
                    "NEON_SAFEKEEPER_TOKEN is required to provision Neon databases".to_string(),
                )
            })?
            .clone();

        Ok(Some(Self::new(
            pageserver_base_url.clone(),
            safekeeper_http_url,
            safekeeper_host_alias,
            safekeeper_pg_port,
            config.neon_bearer_token.clone(),
            safekeeper_bearer_token,
        )))
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
        let start_lsn = self.create_timeline(&tenant_id, &timeline_id).await?;
        self.create_safekeeper_timeline(&tenant_id, &timeline_id, &start_lsn)
            .await?;

        Ok(NeonProvisioning {
            tenant_id,
            timeline_id,
            tenant_gen: TenantGeneration::initial().value() as u32,
        })
    }

    async fn create_tenant(&self, tenant_id: &str) -> DomainResult<()> {
        let clean_tenant_id = tenant_id.replace('-', "");
        let response = self
            .pageserver_request(
                reqwest::Method::PUT,
                &format!("/v1/tenant/{clean_tenant_id}/location_config"),
            )
            .json(&serde_json::json!({
                "mode": TenantLocationMode::AttachedSingle.as_str(),
                "generation": TenantGeneration::initial().value(),
                "tenant_conf": {}
            }))
            .send()
            .await
            .map_err(|e| {
                DomainError::Infrastructure(format!("Neon location config failed: {e}"))
            })?;

        self.ensure_success(response, "init and attach tenant")
            .await?;
        Ok(())
    }

    async fn create_timeline(&self, tenant_id: &str, timeline_id: &str) -> DomainResult<String> {
        let clean_tenant_id = tenant_id.replace('-', "");
        let clean_timeline_id = timeline_id.replace('-', "");
        let response = self
            .pageserver_request(
                reqwest::Method::POST,
                &format!("/v1/tenant/{clean_tenant_id}/timeline"),
            )
            .json(&serde_json::json!({
                "new_timeline_id": clean_timeline_id,
                "pg_version": 16
            }))
            .send()
            .await
            .map_err(|e| {
                DomainError::Infrastructure(format!("Neon timeline request failed: {e}"))
            })?;

        self.ensure_success(response, "create timeline").await?;
        self.fetch_pageserver_last_record_lsn(tenant_id, timeline_id)
            .await
    }

    async fn create_safekeeper_timeline(
        &self,
        tenant_id: &str,
        timeline_id: &str,
        start_lsn: &str,
    ) -> DomainResult<()> {
        let clean_tenant_id = tenant_id.replace('-', "");
        let clean_timeline_id = timeline_id.replace('-', "");
        let response = self
            .safekeeper_request(reqwest::Method::POST, "/v1/tenant/timeline")
            .json(&serde_json::json!({
                "tenant_id": clean_tenant_id,
                "timeline_id": clean_timeline_id,
                "mconf": {
                    "generation": 1,
                    "members": [{
                        "id": 1,
                        "host": self.safekeeper_host_alias,
                        "pg_port": self.safekeeper_pg_port,
                    }],
                    "new_members": null,
                },
                "pg_version": 160000,
                "system_id": null,
                "wal_seg_size": null,
                "start_lsn": start_lsn,
                "commit_lsn": null,
            }))
            .send()
            .await
            .map_err(|e| {
                DomainError::Infrastructure(format!("Safekeeper timeline request failed: {e}"))
            })?;

        self.ensure_success(response, "create safekeeper timeline")
            .await?;
        Ok(())
    }

    async fn fetch_pageserver_last_record_lsn(
        &self,
        tenant_id: &str,
        timeline_id: &str,
    ) -> DomainResult<String> {
        let clean_tenant_id = tenant_id.replace('-', "");
        let clean_timeline_id = timeline_id.replace('-', "");
        let response = self
            .pageserver_request(
                reqwest::Method::GET,
                &format!("/v1/tenant/{clean_tenant_id}/timeline/{clean_timeline_id}"),
            )
            .send()
            .await
            .map_err(|e| {
                DomainError::Infrastructure(format!("Neon timeline detail request failed: {e}"))
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(DomainError::Infrastructure(format!(
                "Neon timeline detail failed: {status} - {body}"
            )));
        }

        let detail: serde_json::Value = response.json().await.map_err(|e| {
            DomainError::Infrastructure(format!("Neon timeline detail response decode failed: {e}"))
        })?;

        detail
            .get("last_record_lsn")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                DomainError::Infrastructure(
                    "Neon timeline detail is missing last_record_lsn".to_string(),
                )
            })
    }

    fn pageserver_request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.request(&self.pageserver_base_url, method, path)
    }

    fn request(
        &self,
        base_url: &str,
        method: reqwest::Method,
        path: &str,
    ) -> reqwest::RequestBuilder {
        let url = format!(
            "{}/{}",
            base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        let builder = HTTP_CLIENT.request(method, url);
        if let Some(token) = &self.pageserver_bearer_token {
            builder.bearer_auth(token)
        } else {
            builder
        }
    }

    fn safekeeper_request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = format!(
            "{}/{}",
            self.safekeeper_http_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        HTTP_CLIENT
            .request(method, url)
            .bearer_auth(&self.safekeeper_bearer_token)
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

    fn parse_safekeeper_connstr(value: &str) -> DomainResult<(String, u16)> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(DomainError::Infrastructure(
                "Empty safekeeper connstring".to_string(),
            ));
        }

        let (host, port) = if let Some(host) = trimmed.strip_prefix('[') {
            let (host, rest) = host.split_once(']').ok_or_else(|| {
                DomainError::Infrastructure(format!("Invalid safekeeper connstring: {trimmed}"))
            })?;
            let port = rest.strip_prefix(':').ok_or_else(|| {
                DomainError::Infrastructure(format!("Invalid safekeeper connstring: {trimmed}"))
            })?;
            (host, port)
        } else {
            trimmed.rsplit_once(':').ok_or_else(|| {
                DomainError::Infrastructure(format!("Invalid safekeeper connstring: {trimmed}"))
            })?
        };

        if host.is_empty() || port.is_empty() {
            return Err(DomainError::Infrastructure(format!(
                "Invalid safekeeper connstring: {trimmed}"
            )));
        }

        let port = port.parse::<u16>().map_err(|e| {
            DomainError::Infrastructure(format!(
                "Invalid safekeeper port in connstring {trimmed}: {e}"
            ))
        })?;

        let alias = if host.contains(':') {
            Self::neon_host_alias("neon-safekeeper", host)
        } else {
            host.to_string()
        };

        Ok((alias, port))
    }

    fn neon_host_alias(prefix: &str, value: &str) -> String {
        let sanitized: String = value
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() {
                    ch.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect();
        format!("{prefix}-{sanitized}")
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
    async fn provision_database_posts_tenant_timeline_and_safekeeper_timeline() {
        let server = MockServer::start().await;
        let token = "jwt-token";
        let safekeeper_host_alias =
            NeonClient::neon_host_alias("neon-safekeeper", "fd40:b90d:fc5f:1ae0::1");

        Mock::given(method("PUT"))
            .and(path(
                "/v1/tenant/11111111111111111111111111111111/location_config",
            ))
            .and(header("authorization", format!("Bearer {token}")))
            .and(body_json(serde_json::json!({
                "mode": "AttachedSingle",
                "generation": 1,
                "tenant_conf": {}
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
                "new_timeline_id": "22222222222222222222222222222222",
                "pg_version": 16
            })))
            .respond_with(ResponseTemplate::new(201))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/v1/tenant/11111111111111111111111111111111/timeline/22222222222222222222222222222222"))
            .and(header("authorization", format!("Bearer {token}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "last_record_lsn": "0/16B6C50"
            })))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/v1/tenant/timeline"))
            .and(body_json(serde_json::json!({
                "tenant_id": "11111111111111111111111111111111",
                "timeline_id": "22222222222222222222222222222222",
                "mconf": {
                    "generation": 1,
                    "members": [{
                        "id": 1,
                        "host": safekeeper_host_alias,
                        "pg_port": 5454
                    }],
                    "new_members": null
                },
                "pg_version": 160000,
                "system_id": null,
                "wal_seg_size": null,
                "start_lsn": "0/16B6C50",
                "commit_lsn": null
            })))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let client = NeonClient::new(
            server.uri(),
            server.uri(),
            safekeeper_host_alias.clone(),
            5454,
            Some(token.to_string()),
            token.to_string(),
        );
        let provisioning = client
            .provision_database_with_ids(
                "11111111111111111111111111111111".to_string(),
                "22222222222222222222222222222222".to_string(),
            )
            .await
            .unwrap();

        assert_eq!(provisioning.tenant_id, "11111111111111111111111111111111");
        assert_eq!(provisioning.timeline_id, "22222222222222222222222222222222");
        assert_eq!(provisioning.tenant_gen, 1);
    }

    #[tokio::test]
    async fn provision_database_uses_distinct_pageserver_and_safekeeper_tokens() {
        let server = MockServer::start().await;
        let pageserver_token = "pageserver-token";
        let safekeeper_token = "safekeeper-token";
        let safekeeper_host_alias =
            NeonClient::neon_host_alias("neon-safekeeper", "fd40:b90d:fc5f:1ae0::1");

        Mock::given(method("PUT"))
            .and(path(
                "/v1/tenant/11111111111111111111111111111111/location_config",
            ))
            .and(header(
                "authorization",
                format!("Bearer {pageserver_token}"),
            ))
            .and(body_json(serde_json::json!({
                "mode": "AttachedSingle",
                "generation": 1,
                "tenant_conf": {}
            })))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/v1/tenant/11111111111111111111111111111111/timeline"))
            .and(header(
                "authorization",
                format!("Bearer {pageserver_token}"),
            ))
            .and(body_json(serde_json::json!({
                "new_timeline_id": "22222222222222222222222222222222",
                "pg_version": 16
            })))
            .respond_with(ResponseTemplate::new(201))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/v1/tenant/11111111111111111111111111111111/timeline/22222222222222222222222222222222"))
            .and(header(
                "authorization",
                format!("Bearer {pageserver_token}"),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "last_record_lsn": "0/16B6C50"
            })))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/v1/tenant/timeline"))
            .and(header(
                "authorization",
                format!("Bearer {safekeeper_token}"),
            ))
            .and(body_json(serde_json::json!({
                "tenant_id": "11111111111111111111111111111111",
                "timeline_id": "22222222222222222222222222222222",
                "mconf": {
                    "generation": 1,
                    "members": [{
                        "id": 1,
                        "host": safekeeper_host_alias,
                        "pg_port": 5454
                    }],
                    "new_members": null
                },
                "pg_version": 160000,
                "system_id": null,
                "wal_seg_size": null,
                "start_lsn": "0/16B6C50",
                "commit_lsn": null
            })))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let client = NeonClient::new(
            server.uri(),
            server.uri(),
            safekeeper_host_alias,
            5454,
            Some(pageserver_token.to_string()),
            safekeeper_token.to_string(),
        );

        client
            .provision_database_with_ids(
                "11111111111111111111111111111111".to_string(),
                "22222222222222222222222222222222".to_string(),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn from_config_requires_safekeeper_token_when_neon_is_enabled() {
        let config = ApiConfig {
            neon_pageserver_url: Some("http://[fd40:b90d:fc5f:1ae0::1]:9898".to_string()),
            neon_safekeeper_http_url: Some("http://[fd40:b90d:fc5f:1ae0::1]:7676".to_string()),
            neon_safekeeper_connstrs: Some("[fd40:b90d:fc5f:1ae0::1]:5454".to_string()),
            neon_safekeeper_token: None,
            ..Default::default()
        };

        let err = NeonClient::from_config(&config).unwrap_err();
        let message = match err {
            DomainError::Infrastructure(message) => message,
            other => panic!("expected infrastructure error, got {other:?}"),
        };

        assert!(message.contains("NEON_SAFEKEEPER_TOKEN"));
    }

    #[tokio::test]
    async fn from_config_requires_safekeeper_http_url_when_neon_is_enabled() {
        let config = ApiConfig {
            neon_pageserver_url: Some("http://[fd40:b90d:fc5f:1ae0::1]:9898".to_string()),
            neon_safekeeper_http_url: None,
            neon_safekeeper_connstrs: Some("[fd40:b90d:fc5f:1ae0::1]:5454".to_string()),
            neon_safekeeper_token: Some("token-123".to_string()),
            ..Default::default()
        };

        let err = NeonClient::from_config(&config).unwrap_err();
        let message = match err {
            DomainError::Infrastructure(message) => message,
            other => panic!("expected infrastructure error, got {other:?}"),
        };

        assert!(message.contains("NEON_SAFEKEEPER_HTTP_URL"));
    }

    #[test]
    fn attached_single_location_config_is_explicit() {
        let config = TenantLocationConfig::attached_single(TenantGeneration::new(42));
        assert_eq!(config.mode, TenantLocationMode::AttachedSingle);
        assert_eq!(config.generation, TenantGeneration::new(42));
    }

    #[tokio::test]
    async fn provision_database_strips_hyphens_from_tenant_location_path() {
        let server = MockServer::start().await;
        let token = "jwt-token";

        Mock::given(method("PUT"))
            .and(path(
                "/v1/tenant/11111111111111111111111111111111/location_config",
            ))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/v1/tenant/11111111111111111111111111111111/timeline"))
            .and(body_json(serde_json::json!({
                "new_timeline_id": "22222222222222222222222222222222",
                "pg_version": 16
            })))
            .respond_with(ResponseTemplate::new(201))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/v1/tenant/11111111111111111111111111111111/timeline/22222222222222222222222222222222"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "last_record_lsn": "0/0"
            })))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/v1/tenant/timeline"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let client = NeonClient::new(
            server.uri(),
            server.uri(),
            "neon-safekeeper-fd40-b90d-fc5f-1ae0--1".to_string(),
            5454,
            Some(token.to_string()),
            token.to_string(),
        );

        let provisioning = client
            .provision_database_with_ids(
                "11111111111111111111111111111111".to_string(),
                "22222222222222222222222222222222".to_string(),
            )
            .await
            .unwrap();

        assert_eq!(provisioning.tenant_id, "11111111111111111111111111111111");
    }
}
