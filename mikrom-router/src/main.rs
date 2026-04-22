use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Request, StatusCode},
    response::IntoResponse,
    routing::any,
};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use moka::future::Cache;
use sqlx::PgPool;
use tracing::{error, info};

mod config;

#[derive(Clone)]
struct AppState {
    db: PgPool,
    cache: Cache<String, String>, // Hostname -> internal IP:Port
    #[allow(dead_code)]
    config: config::Config,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = config::Config::from_env().expect("Failed to load config");

    tracing_subscriber::fmt()
        .with_env_filter(&config.log_level)
        .init();

    let db = PgPool::connect(&config.database_url).await?;

    let cache = Cache::builder()
        .max_capacity(1000)
        .time_to_live(std::time::Duration::from_secs(60))
        .build();

    let state = AppState {
        db,
        cache,
        config: config.clone(),
    };

    let app = Router::new().fallback(any(proxy_handler)).with_state(state);

    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!("Mikrom Router listening on {}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}

async fn proxy_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut req: Request<Body>,
) -> impl IntoResponse {
    // 1. Get host from headers and normalize (remove port)
    let raw_host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    let host = raw_host.split(':').next().unwrap_or(raw_host);

    // 2. Resolve host to internal target
    let target_url = match resolve_target(&state, host).await {
        Ok(url) => url,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };

    info!("Proxying request for {} to {}", host, target_url);

    // 3. Perform proxying
    let client = hyper_util::client::legacy::Client::builder(TokioExecutor::new())
        .build(HttpConnector::new());

    // Construct the backend URL
    let path_query = req.uri().path_and_query().map(|v| v.as_str()).unwrap_or("");
    let full_target = format!("{}{}", target_url, path_query);

    match full_target.parse::<hyper::Uri>() {
        Ok(uri) => {
            *req.uri_mut() = uri;
            match client.request(req).await {
                Ok(res) => res.into_response(),
                Err(e) => {
                    error!("Proxy error: {}", e);
                    StatusCode::BAD_GATEWAY.into_response()
                }
            }
        }
        Err(e) => {
            error!("Invalid target URI {}: {}", full_target, e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn resolve_target(state: &AppState, host: &str) -> anyhow::Result<String> {
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

    Err(anyhow::anyhow!("Host not found"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_logic() {
        let cache = Cache::builder().build();
        let host = "test.apps.mikrom.es".to_string();
        let target = "http://10.0.2.2:80".to_string();

        cache.insert(host.clone(), target.clone()).await;

        assert_eq!(cache.get(&host).await.unwrap(), target);
    }
}
