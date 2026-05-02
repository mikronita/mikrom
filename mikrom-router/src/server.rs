use crate::AppState;
use crate::acme::acme_challenge_handler;
use crate::resolver::resolve_target;
use crate::tls::DatabaseCertResolver;
use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Request, StatusCode},
    response::{IntoResponse, Redirect},
    routing::{any, get},
};
use sqlx::PgPool;
use std::sync::Arc;
use tokio_rustls::rustls;
use tracing::{error, info};

pub async fn start_http_server(
    state: AppState,
    host: String,
    port: u16,
    https_port: u16,
) -> anyhow::Result<()> {
    let http_app = Router::new()
        .route(
            "/.well-known/acme-challenge/{token}",
            get(acme_challenge_handler),
        )
        .fallback(move |headers: HeaderMap| async move {
            let host = headers
                .get("host")
                .and_then(|h| h.to_str().ok())
                .map(|h| h.split_once(':').map_or(h, |(h, _)| h))
                .unwrap_or("localhost");

            let redirect_url = format!("https://{}:{}/", host, https_port);
            Redirect::permanent(&redirect_url)
        })
        .with_state(state);

    let http_addr = format!("{}:{}", host, port);
    info!("HTTP Router listening on {}", http_addr);
    let http_listener = std::net::TcpListener::bind(&http_addr)?;

    tokio::spawn(async move {
        if let Err(e) = axum_server::from_tcp(http_listener)
            .serve(http_app.into_make_service())
            .await
        {
            error!("HTTP server error: {}", e);
        }
    });

    Ok(())
}

pub async fn start_https_server(
    state: AppState,
    db: PgPool,
    host: String,
    port: u16,
    master_key: String,
    cache_ttl: u64,
) -> anyhow::Result<()> {
    let https_app = Router::new()
        .route("/health", any(health_handler))
        .fallback(any(proxy_handler))
        .with_state(state);

    let resolver = Arc::new(DatabaseCertResolver::new(db, master_key, cache_ttl));
    let mut server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(resolver);

    server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    let https_addr = format!("{}:{}", host, port);
    info!("HTTPS Router listening on {}", https_addr);
    let https_listener = std::net::TcpListener::bind(&https_addr)?;

    let tls_config = axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(server_config));

    axum_server::from_tcp(https_listener)
        .acceptor(axum_server::tls_rustls::RustlsAcceptor::new(tls_config))
        .serve(https_app.into_make_service())
        .await?;

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
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .map(|h| h.split_once(':').map_or(h, |(h, _)| h))
        .unwrap_or("unknown");

    let target_url = match resolve_target(&state, host).await {
        Ok(url) => url,
        Err(e) => {
            info!("Host resolution failed for {}: {}", host, e);
            return StatusCode::NOT_FOUND.into_response();
        },
    };

    info!("Proxying request for {} to {}", host, target_url);

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
