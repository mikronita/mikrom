use moka::future::Cache;
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub cache: Cache<String, String>, // Hostname -> internal IP:Port
}

pub async fn resolve_target(state: &AppState, host: &str) -> anyhow::Result<String> {
    // Check cache first
    if let Some(target) = state.cache.get(host).await {
        return Ok(target);
    }

    // Lookup in DB: join apps and deployments to find the RUNNING VM's IP
    let row = sqlx::query(
        r#"
        SELECT a.port, d.ip_address
        FROM apps a
        JOIN deployments d ON a.id = d.app_id
        WHERE a.hostname = $1 AND d.status = 'RUNNING' AND d.ip_address IS NOT NULL
        ORDER BY d.created_at DESC
        LIMIT 1
        "#,
    )
    .bind(host)
    .fetch_optional(&state.db)
    .await?;

    if let Some(row) = row {
        use sqlx::Row;
        let port: i32 = row.get("port");
        let ip: String = row.get("ip_address");

        let target = format!("http://{}:{}", ip, port);

        state.cache.insert(host.to_string(), target.clone()).await;
        return Ok(target);
    }

    Err(anyhow::anyhow!("Host not found: {}", host))
}

#[cfg(test)]
mod tests {
    use super::*;
    use moka::future::Cache;

    #[tokio::test]
    async fn test_resolve_target_from_cache() {
        let cache = Cache::builder().build();
        let host = "test.example.com";
        let target = "http://1.2.3.4:8080";
        cache.insert(host.to_string(), target.to_string()).await;

        // We don't need a real DB pool if it hits the cache
        let state = AppState {
            db: PgPool::connect_lazy("postgres://localhost/test").unwrap(),
            cache,
        };

        let result = resolve_target(&state, host).await.unwrap();
        assert_eq!(result, target);
    }
}
