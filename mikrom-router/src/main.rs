use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Request, StatusCode},
    response::{IntoResponse, Redirect},
    routing::{any, get},
};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use mikrom_proto::router::RouterConfigUpdate;
use mikrom_router::acme::acme_challenge_handler;
use mikrom_router::tls::DatabaseCertResolver;
use mikrom_router::{AppState, resolve_target};
use moka::future::Cache;
use prost::Message;
use sqlx::PgPool;
use std::sync::Arc;
use tokio_rustls::rustls;
use tokio_stream::StreamExt;
use tracing::{error, info};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Install the default crypto provider for Rustls 0.23
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

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

    let state = AppState {
        db: db.clone(),
        cache,
        client,
    };

    // 2. Background task for router updates (Routes, TLS, ACME)
    let cache_clone = state.cache.clone();
    let db_clone = state.db.clone();
    let nats_url = config.nats_url.clone();

    tokio::spawn(async move {
        loop {
            info!("Connecting to NATS for updates at {}...", nats_url);
            let nats_client = match async_nats::connect(&nats_url).await {
                Ok(client) => client,
                Err(e) => {
                    error!("Failed to connect to NATS, retrying in 5s: {}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                },
            };

            // Subscribe to all router-related updates
            let config_sub = nats_client
                .subscribe(mikrom_proto::subjects::ROUTER_CONFIG_UPDATED)
                .await;
            let tls_sub = nats_client
                .subscribe(mikrom_proto::subjects::ROUTER_TLS_CERT_UPDATED)
                .await;
            let acme_sub = nats_client
                .subscribe(mikrom_proto::subjects::ROUTER_ACME_CHALLENGE_UPDATED)
                .await;

            let (mut config_sub, mut tls_sub, mut acme_sub) = match (config_sub, tls_sub, acme_sub)
            {
                (Ok(c), Ok(t), Ok(a)) => (c, t, a),
                _ => {
                    error!("Failed to subscribe to one or more NATS subjects, retrying in 5s");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                },
            };

            info!("Listening for router config, TLS, and ACME updates via NATS...");

            loop {
                tokio::select! {
                    Some(msg) = config_sub.next() => {
                        if let Ok(update) = RouterConfigUpdate::decode(&msg.payload[..]) {
                            info!("Received router update for {}: {:?}", update.hostname, update.target_url);
                            let result = if let Some(target) = update.target_url {
                                sqlx::query("INSERT INTO routes (hostname, target_url, updated_at) VALUES ($1, $2, TO_TIMESTAMP($3)) ON CONFLICT (hostname) DO UPDATE SET target_url = EXCLUDED.target_url, updated_at = EXCLUDED.updated_at WHERE EXCLUDED.updated_at > routes.updated_at")
                                    .bind(&update.hostname)
                                    .bind(&target)
                                    .bind(update.timestamp)
                                    .execute(&db_clone)
                                    .await
                            } else {
                                sqlx::query("DELETE FROM routes WHERE hostname = $1 AND updated_at <= TO_TIMESTAMP($2)")
                                    .bind(&update.hostname)
                                    .bind(update.timestamp)
                                    .execute(&db_clone)
                                    .await
                            };

                            if let Err(e) = result {
                                error!("Failed to update local routes table: {}", e);
                            } else {
                                cache_clone.invalidate(&update.hostname).await;
                            }
                        }
                    },
                    Some(msg) = tls_sub.next() => {
                        use mikrom_proto::router::TlsCertificateUpdate;
                        if let Ok(update) = TlsCertificateUpdate::decode(&msg.payload[..]) {
                            info!("Received TLS certificate update for {}", update.hostname);
                            let result = sqlx::query("INSERT INTO tls_certificates (hostname, cert_chain, private_key, expires_at) VALUES ($1, $2, $3, TO_TIMESTAMP($4)) ON CONFLICT (hostname) DO UPDATE SET cert_chain = EXCLUDED.cert_chain, private_key = EXCLUDED.private_key, expires_at = EXCLUDED.expires_at, updated_at = NOW()")
                                .bind(&update.hostname)
                                .bind(&update.cert_chain)
                                .bind(&update.private_key)
                                .bind(update.expires_at)
                                .execute(&db_clone)
                                .await;

                            if let Err(e) = result {
                                error!("Failed to update local tls_certificates table: {}", e);
                            } else {
                                cache_clone.invalidate(&update.hostname).await;
                            }
                        }
                    },
                    Some(msg) = acme_sub.next() => {
                        use mikrom_proto::router::AcmeChallengeUpdate;
                        if let Ok(update) = AcmeChallengeUpdate::decode(&msg.payload[..]) {
                            info!("Received ACME challenge update for token: {}", update.token);
                            let result = if update.is_delete {
                                sqlx::query("DELETE FROM acme_challenges WHERE token = $1")
                                    .bind(&update.token)
                                    .execute(&db_clone)
                                    .await
                            } else {
                                sqlx::query("INSERT INTO acme_challenges (token, key_auth, hostname) VALUES ($1, $2, $3) ON CONFLICT (token) DO UPDATE SET key_auth = EXCLUDED.key_auth, hostname = EXCLUDED.hostname")
                                    .bind(&update.token)
                                    .bind(&update.key_auth)
                                    .bind(&update.hostname)
                                    .execute(&db_clone)
                                    .await
                            };

                            if let Err(e) = result {
                                error!("Failed to update local acme_challenges table: {}", e);
                            }
                        }
                    },
                    else => break,
                }
            }
            error!("NATS subscription closed, reconnecting in 5s...");
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });

    // 3. HTTP Server (Redirects to HTTPS + ACME Challenges)
    let http_state = state.clone();
    let https_port = config.https_port;
    let http_app = Router::new()
        .route(
            "/.well-known/acme-challenge/{token}",
            get(acme_challenge_handler),
        )
        .fallback(move |headers: HeaderMap| async move {
            let host = headers
                .get("host")
                .and_then(|h| h.to_str().ok())
                .map(|h| h.split(':').next().unwrap_or(h))
                .unwrap_or("localhost");

            let redirect_url = format!("https://{}:{}/", host, https_port);
            Redirect::permanent(&redirect_url)
        })
        .with_state(http_state);

    let http_addr = format!("{}:{}", config.host, config.http_port);
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

    // 4. HTTPS Server (Main Proxy)
    let https_app = Router::new()
        .route("/health", any(health_handler))
        .fallback(any(proxy_handler))
        .with_state(state);

    let resolver = Arc::new(DatabaseCertResolver::new(
        db.clone(),
        config.master_key.clone(),
        config.cache_ttl,
    ));
    let mut server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(resolver);

    // Support HTTP/2 and HTTP/1.1
    server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    let https_addr = format!("{}:{}", config.host, config.https_port);
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
