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
use mikrom_router::{AppState, RouterConfig, resolve_target};
use moka::future::Cache;
use sqlx::PgPool;
use tokio_stream::StreamExt;
use tracing::{error, info};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = mikrom_router::config::Config::from_env().expect("Failed to load config");

    mikrom_proto::telemetry::init_telemetry("mikrom-router", "0.1.0")?;

    info!("Connecting to database...");
    let db = PgPool::connect(&config.database_url).await?;

    info!("Running database migrations...");
    sqlx::migrate!("./migrations").run(&db).await?;

    let cache = Cache::builder()
        .max_capacity(1000)
        .time_to_live(std::time::Duration::from_secs(60))
        .build();

    let client = hyper_util::client::legacy::Client::builder(TokioExecutor::new())
        .build(HttpConnector::new());

    // NATS client for cache invalidation and updates
    info!("Connecting to NATS at {}...", config.nats_url);
    let nats_client = async_nats::connect(&config.nats_url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to NATS: {}", e))?;

    let state = AppState {
        db: db.clone(),
        cache,
        client,
    };

    // Background task to listen for router configuration updates
    let cache_clone = state.cache.clone();
    let db_clone = state.db.clone();
    let mut nats_sub = nats_client
        .subscribe("mikrom.router.config_updated")
        .await?;

    tokio::spawn(async move {
        info!("Listening for router config updates via NATS...");
        while let Some(msg) = nats_sub.next().await {
            if let Ok(update) = serde_json::from_slice::<RouterConfig>(&msg.payload) {
                info!(
                    "Received router update for {}: {:?}",
                    update.hostname, update.target_url
                );

                let result = if let Some(target) = update.target_url {
                    sqlx::query("INSERT INTO routes (hostname, target_url, updated_at) VALUES ($1, $2, NOW()) ON CONFLICT (hostname) DO UPDATE SET target_url = EXCLUDED.target_url, updated_at = NOW()")
                        .bind(&update.hostname)
                        .bind(&target)
                        .execute(&db_clone)
                        .await
                } else {
                    sqlx::query("DELETE FROM routes WHERE hostname = $1")
                        .bind(&update.hostname)
                        .execute(&db_clone)
                        .await
                };

                if let Err(e) = result {
                    error!("Failed to update local routes table: {}", e);
                } else {
                    cache_clone.invalidate(&update.hostname).await;
                }
            } else {
                // Legacy or invalid message: clear everything to be safe
                info!("Received invalid or legacy update, clearing router cache");
                cache_clone.invalidate_all();
            }
        }
    });

    let app = Router::new()
        .route("/health", any(health_handler))
        .fallback(any(proxy_handler))
        .with_state(state);

    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!("Mikrom Router listening on {}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_handler() -> impl IntoResponse {
    StatusCode::OK
}

async fn proxy_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut req: Request<Body>,
) -> impl IntoResponse {
    // 1. Get host from headers and normalize (remove port)
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .map(|h| h.split(':').next().unwrap_or(h))
        .unwrap_or("unknown");

    // 2. Resolve host to internal target
    let target_url = match resolve_target(&state, host).await {
        Ok(url) => url,
        Err(e) => {
            info!("Host resolution failed for {}: {}", host, e);
            return StatusCode::NOT_FOUND.into_response();
        },
    };

    info!("Proxying request for {} to {}", host, target_url);

    // 3. Perform proxying
    // Construct the backend URL
    let path_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("");
    let full_url = format!("{}{}", target_url, path_query);

    match hyper::Uri::try_from(full_url) {
        Ok(uri) => {
            *req.uri_mut() = uri;
            match state.client.request(req).await {
                Ok(resp) => resp.into_response(),
                Err(e) => {
                    error!("Proxy request failed: {}", e);
                    StatusCode::BAD_GATEWAY.into_response()
                },
            }
        },
        Err(e) => {
            error!("Invalid target URI: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        },
    }
}
