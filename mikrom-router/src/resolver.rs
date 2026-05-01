use axum::body::Body;
use hyper_util::client::legacy::connect::HttpConnector;
use moka::future::Cache;
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub cache: Cache<String, String>, // Hostname -> target_url
    pub client: hyper_util::client::legacy::Client<HttpConnector, Body>,
}

pub async fn resolve_target(state: &AppState, host: &str) -> anyhow::Result<String> {
    // Check cache first
    if let Some(target) = state.cache.get(host).await {
        return Ok(target);
    }

    // Lookup in local routes table
    let row = sqlx::query(
        r#"
        SELECT target_url FROM routes WHERE hostname = $1
        "#,
    )
    .bind(host)
    .fetch_optional(&state.db)
    .await?;

    if let Some(row) = row {
        use sqlx::Row;
        let target: String = row.get("target_url");

        state.cache.insert(host.to_string(), target.clone()).await;
        return Ok(target);
    }

    Err(anyhow::anyhow!("Host not found: {}", host))
}

#[cfg(test)]
#[path = "../tests/common_utils.rs"]
mod common_utils;

#[cfg(test)]
mod tests {
    use super::common_utils;
    use super::*;
    use moka::future::Cache;

    #[tokio::test]
    async fn test_resolve_target_from_cache() {
        let cache = Cache::builder().build();
        let host = "test.example.com";
        let target = "http://1.2.3.4:8080";
        cache.insert(host.to_string(), target.to_string()).await;

        let test_db = common_utils::TestDb::new().await;
        let state = AppState {
            db: test_db.pool().clone(),
            cache,
            client: hyper_util::client::legacy::Client::builder(
                hyper_util::rt::TokioExecutor::new(),
            )
            .build(hyper_util::client::legacy::connect::HttpConnector::new()),
        };

        let result = resolve_target(&state, host).await.unwrap();
        assert_eq!(result, target);
    }
}
