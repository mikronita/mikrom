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
mod resolver;

use resolver::{AppState, resolve_target};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = config::Config::from_env().expect("Failed to load config");

    mikrom_proto::telemetry::init_telemetry("mikrom-router", "0.1.0")?;

    let db = PgPool::connect(&config.database_url).await?;

    let cache = Cache::builder()
        .max_capacity(1000)
        .time_to_live(std::time::Duration::from_secs(60))
        .build();

    let client = hyper_util::client::legacy::Client::builder(TokioExecutor::new())
        .build(HttpConnector::new());

    let state = AppState { db, cache, client };

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
    let path_query = req.uri().path_and_query().map(|v| v.as_str()).unwrap_or("");
    let full_target = format!("{}{}", target_url, path_query);

    match full_target.parse::<hyper::Uri>() {
        Ok(uri) => {
            *req.uri_mut() = uri;
            match state.client.request(req).await {
                Ok(res) => res.into_response(),
                Err(e) => {
                    error!("Proxy error: {}", e);
                    StatusCode::BAD_GATEWAY.into_response()
                },
            }
        },
        Err(e) => {
            error!("Invalid target URI {}: {}", full_target, e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        },
    }
}
