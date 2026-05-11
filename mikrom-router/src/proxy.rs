use crate::state::State;
use async_trait::async_trait;
use pingora::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use tokio::sync::RwLock;

pub struct RouterMetricsCounters {
    pub requests_total: AtomicU64,
    pub responses_2xx: AtomicU64,
    pub responses_4xx: AtomicU64,
    pub responses_5xx: AtomicU64,
}

impl RouterMetricsCounters {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            requests_total: AtomicU64::new(0),
            responses_2xx: AtomicU64::new(0),
            responses_4xx: AtomicU64::new(0),
            responses_5xx: AtomicU64::new(0),
        }
    }
}

impl Default for RouterMetricsCounters {
    fn default() -> Self {
        Self::new()
    }
}

pub struct MikromProxy {
    state: Arc<RwLock<State>>,
    lb_counter: AtomicUsize,
    acme_staging: bool,
    pub metrics: Arc<RouterMetricsCounters>,
}

impl MikromProxy {
    pub const fn new(
        state: Arc<RwLock<State>>,
        acme_staging: bool,
        metrics: Arc<RouterMetricsCounters>,
    ) -> Self {
        Self {
            state,
            lb_counter: AtomicUsize::new(0),
            acme_staging,
            metrics,
        }
    }

    pub async fn select_target(&self, host: &str) -> Result<String> {
        let state = self.state.read().await;

        if let Some(route) = state.routes.get(host) {
            if route.targets.is_empty() {
                return Err(Error::explain(
                    ErrorType::InternalError,
                    format!("No targets defined for host: {host}"),
                ));
            }

            // Simple Round Robin
            let index = self.lb_counter.fetch_add(1, Ordering::Relaxed) % route.targets.len();
            let target = route.targets[index].clone();
            drop(state);
            return Ok(target);
        }

        drop(state);
        Err(Error::explain(
            ErrorType::HTTPStatus(404),
            format!("No route found for host: {host}"),
        ))
    }
}

#[async_trait]
impl ProxyHttp for MikromProxy {
    type CTX = ();
    fn new_ctx(&self) -> Self::CTX {}

    async fn request_filter(&self, session: &mut Session, _ctx: &mut Self::CTX) -> Result<bool> {
        let host = session
            .get_header("Host")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");

        let path = session.req_header().uri.path();

        // 1. Handle ACME Challenge
        if path.starts_with("/.well-known/acme-challenge/") {
            let token = path.strip_prefix("/.well-known/acme-challenge/").unwrap();
            let state = self.state.read().await;

            if let Some(key_auth) = state.acme_tokens.get(token) {
                if self.acme_staging {
                    tracing::info!(
                        "ACME challenge received for host {host}: token={token}, responding with key_auth"
                    );
                }

                let mut response = ResponseHeader::build(200, Some(key_auth.len()))?;
                response.insert_header("Content-Type", "text/plain")?;

                session
                    .write_response_header(Box::new(response), false)
                    .await?;
                session
                    .write_response_body(Some(key_auth.clone().into()), true)
                    .await?;
                return Ok(true); // Short-circuit
            } else if self.acme_staging {
                tracing::warn!(
                    "ACME challenge received for host {host} but token {token} not found in state"
                );
            }
        }

        // 2. HTTP to HTTPS Redirection
        // If we are not on TLS and the host has a certificate, redirect to 443
        let is_tls = session
            .downstream_session
            .digest()
            .is_some_and(|d| d.ssl_digest.is_some());

        if !is_tls {
            let state = self.state.read().await;
            if state.certificates.contains_key(host) {
                let mut redirect = ResponseHeader::build(301, None)?;
                let location = format!("https://{host}{path}");
                redirect.insert_header("Location", location)?;
                session
                    .write_response_header(Box::new(redirect), true)
                    .await?;
                return Ok(true);
            }
        }

        Ok(false)
    }

    async fn upstream_peer(
        &self,
        session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        let host = session
            .get_header("Host")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");

        let target = self.select_target(host).await?;
        let peer = Box::new(HttpPeer::new(&target, false, host.to_string()));
        Ok(peer)
    }

    async fn upstream_request_filter(
        &self,
        session: &mut Session,
        upstream_request: &mut RequestHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<()> {
        // Add standard proxy headers
        if let Some(addr) = session.client_addr()
            && let Some(inet) = addr.as_inet()
        {
            let ip = inet.ip().to_string();
            upstream_request.insert_header("X-Forwarded-For", &ip)?;
            upstream_request.insert_header("X-Real-IP", &ip)?;
        }

        let is_tls = session
            .downstream_session
            .digest()
            .is_some_and(|d| d.ssl_digest.is_some());

        if is_tls {
            upstream_request.insert_header("X-Forwarded-Proto", "https")?;
        } else {
            upstream_request.insert_header("X-Forwarded-Proto", "http")?;
        }

        Ok(())
    }

    async fn logging(&self, session: &mut Session, _e: Option<&Error>, _ctx: &mut Self::CTX) {
        self.metrics.requests_total.fetch_add(1, Ordering::Relaxed);

        if let Some(response) = session.response_written() {
            let code = response.status.as_u16();
            if (200..300).contains(&code) {
                self.metrics.responses_2xx.fetch_add(1, Ordering::Relaxed);
            } else if (400..500).contains(&code) {
                self.metrics.responses_4xx.fetch_add(1, Ordering::Relaxed);
            } else if (500..600).contains(&code) {
                self.metrics.responses_5xx.fetch_add(1, Ordering::Relaxed);
            }
        } else {
            self.metrics.responses_5xx.fetch_add(1, Ordering::Relaxed);
        }
    }
}
