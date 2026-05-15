use crate::state::State;
use async_trait::async_trait;
use openssl::x509::X509;
use opentelemetry::propagation::{Extractor, Injector};
use pingora::lb::LoadBalancer;
use pingora::lb::selection::RoundRobin;
use pingora::modules::http::HttpModules;
use pingora::modules::http::compression::ResponseCompressionBuilder;
use pingora::prelude::*;
use pingora_limits::rate::Rate;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{info, warn};
use tracing_opentelemetry::OpenTelemetrySpanExt;

fn canonical_host(host: &str) -> String {
    if let Some(rest) = host.strip_prefix('[')
        && let Some((ipv6, suffix)) = rest.split_once(']')
        && (suffix.is_empty() || suffix.starts_with(':'))
    {
        return format!("[{ipv6}]");
    }

    if let Some((name, port)) = host.rsplit_once(':')
        && !name.contains(':')
        && !port.contains(':')
    {
        return name.to_string();
    }

    host.to_string()
}

pub struct RouterMetricsCounters {
    pub requests_total: AtomicU64,
    pub responses_2xx: AtomicU64,
    pub responses_3xx: AtomicU64,
    pub responses_4xx: AtomicU64,
    pub responses_5xx: AtomicU64,
    pub latency_sum_ms: AtomicU64,
}

impl RouterMetricsCounters {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            requests_total: AtomicU64::new(0),
            responses_2xx: AtomicU64::new(0),
            responses_3xx: AtomicU64::new(0),
            responses_4xx: AtomicU64::new(0),
            responses_5xx: AtomicU64::new(0),
            latency_sum_ms: AtomicU64::new(0),
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
    acme_staging: bool,
    upstream_ca: Option<Arc<Box<[X509]>>>,
    pub metrics: Arc<RouterMetricsCounters>,
    rate_limiter: Rate,
    rps_limit: isize,
}

pub struct MikromCtx {
    pub span: tracing::Span,
    pub request_start_time: chrono::DateTime<chrono::Utc>,
}

impl MikromProxy {
    #[must_use]
    pub fn new(
        state: Arc<RwLock<State>>,
        acme_staging: bool,
        upstream_ca: Option<Arc<Box<[X509]>>>,
        metrics: Arc<RouterMetricsCounters>,
        rps_limit: isize,
    ) -> Self {
        Self {
            state,
            acme_staging,
            upstream_ca,
            metrics,
            rate_limiter: Rate::new(Duration::from_secs(1)),
            rps_limit,
        }
    }

    pub async fn get_lb_and_tls(
        &self,
        host: &str,
    ) -> Result<(Arc<LoadBalancer<RoundRobin>>, bool, Option<String>)> {
        let raw_host = host;
        let host = canonical_host(raw_host);
        let state = self.state.read().await;

        let res = state
            .routes
            .get(host.as_str())
            .or_else(|| state.routes.get(raw_host));
        let res = res.map_or_else(
            || {
                Err(Error::explain(
                    ErrorType::HTTPStatus(404),
                    format!("No route found for host: {host}"),
                ))
            },
            |route| {
                Ok((
                    route.lb.clone(),
                    route.use_tls,
                    route.tls_alternative_cn.clone(),
                ))
            },
        );
        drop(state);
        res
    }

    pub async fn get_lb(&self, host: &str) -> Result<Arc<LoadBalancer<RoundRobin>>> {
        self.get_lb_and_tls(host).await.map(|(lb, _, _)| lb)
    }

    async fn write_text_response(
        session: &mut Session,
        status: u16,
        headers: &[(&str, &str)],
        body: &str,
        end_stream: bool,
    ) -> Result<bool> {
        let mut response = ResponseHeader::build(status, Some(body.len()))?;
        for (key, value) in headers {
            response.insert_header((*key).to_string(), (*value).to_string())?;
        }

        session
            .write_response_header(Box::new(response), end_stream)
            .await?;
        session
            .write_response_body(Some(body.to_string().into()), true)
            .await?;
        Ok(true)
    }

    async fn maybe_handle_acme_challenge(
        &self,
        session: &mut Session,
        host: &str,
        path: &str,
    ) -> Result<Option<bool>> {
        let Some(token) = path.strip_prefix("/.well-known/acme-challenge/") else {
            return Ok(None);
        };

        let key_auth = {
            let state = self.state.read().await;
            state.acme_tokens.get(token).cloned()
        };

        if let Some(key_auth) = key_auth {
            if self.acme_staging {
                tracing::info!(
                    "ACME challenge received for host {host}: token={token}, responding with key_auth"
                );
            }

            let result = Self::write_text_response(
                session,
                200,
                &[("Content-Type", "text/plain")],
                &key_auth,
                false,
            )
            .await?;
            return Ok(Some(result));
        }

        if self.acme_staging {
            tracing::warn!(
                "ACME challenge received for host {host} but token {token} not found in state"
            );
        }

        Ok(None)
    }

    async fn maybe_redirect_http(
        &self,
        session: &mut Session,
        normalized_host: &str,
        path: &str,
    ) -> Result<Option<bool>> {
        let is_tls = session
            .downstream_session
            .digest()
            .is_some_and(|d| d.ssl_digest.is_some());

        if is_tls {
            return Ok(None);
        }

        let has_certificate = {
            let state = self.state.read().await;
            state.certificates.contains_key(normalized_host)
        };

        if !has_certificate {
            return Ok(None);
        }

        let location = format!("https://{normalized_host}{path}");
        let mut redirect = ResponseHeader::build(301, None)?;
        redirect.insert_header("Location", location)?;
        session
            .write_response_header(Box::new(redirect), true)
            .await?;
        Ok(Some(true))
    }
}

struct PingoraHeaderInjector<'a>(&'a mut RequestHeader);

impl Injector for PingoraHeaderInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        use http::header::HeaderName;
        match HeaderName::try_from(key) {
            Ok(name) => {
                if let Err(e) = self.0.insert_header(name, value) {
                    warn!("Failed to inject tracing header {key}: {e}");
                }
            },
            Err(e) => warn!("Invalid tracing header key {key}: {e}"),
        }
    }
}

struct PingoraHeaderExtractor<'a>(&'a RequestHeader);

impl Extractor for PingoraHeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.headers.get(key).and_then(|h| h.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0
            .headers
            .keys()
            .map(http::HeaderName::as_str)
            .collect()
    }
}

#[async_trait]
impl ProxyHttp for MikromProxy {
    type CTX = MikromCtx;
    fn new_ctx(&self) -> Self::CTX {
        MikromCtx {
            span: tracing::Span::none(),
            request_start_time: chrono::Utc::now(),
        }
    }

    fn init_downstream_modules(&self, modules: &mut HttpModules) {
        modules.add_module(ResponseCompressionBuilder::enable(3));
    }

    async fn request_filter(&self, session: &mut Session, ctx: &mut Self::CTX) -> Result<bool> {
        // 0. Extract Tracing Context and Start Span
        let parent_cx = opentelemetry::global::get_text_map_propagator(|propagator| {
            propagator.extract(&PingoraHeaderExtractor(session.req_header()))
        });

        let span = tracing::info_span!("proxy_request", 
            method = ?session.req_header().method,
            uri = %session.req_header().uri);
        span.set_parent(parent_cx);
        ctx.span = span;

        // 0.1 Rate Limiting (Per IP)
        if let Some(addr) = session.client_addr() {
            let ip = addr.to_string();
            let curr_window_requests = self.rate_limiter.observe(&ip, 1);
            if curr_window_requests > self.rps_limit {
                warn!("Rate limit exceeded for IP: {ip} (requests: {curr_window_requests})");
                return Self::write_text_response(
                    session,
                    429,
                    &[("Content-Type", "text/plain"), ("Retry-After", "1")],
                    "Too Many Requests\n",
                    false,
                )
                .await;
            }
        }

        let host = session
            .get_header("Host")
            .and_then(|h| h.to_str().ok())
            .or_else(|| session.req_header().uri.host())
            .unwrap_or("")
            .to_string();
        let normalized_host = canonical_host(&host);

        let path = session.req_header().uri.path().to_string();

        if self
            .maybe_handle_acme_challenge(session, &host, &path)
            .await?
            == Some(true)
        {
            return Ok(true);
        }

        if self
            .maybe_redirect_http(session, normalized_host.as_str(), path.as_str())
            .await?
            == Some(true)
        {
            return Ok(true);
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
            .or_else(|| session.req_header().uri.host())
            .unwrap_or("");
        let normalized_host = canonical_host(host);

        let (lb, use_tls, alternative_cn) = self.get_lb_and_tls(host).await?;

        // Use client address as a hash seed for better distribution/stickiness if LB supports it
        let hash = session
            .client_addr()
            .map_or_else(|| b"".to_vec(), |addr| addr.to_string().into_bytes());

        let upstream = lb.select(&hash, 256).ok_or_else(|| {
            Error::explain(
                ErrorType::InternalError,
                format!("No healthy upstreams for host: {normalized_host}"),
            )
        })?;

        info!("Selected upstream: {upstream:?}, use_tls: {use_tls}");
        let mut peer = HttpPeer::new(upstream.to_string(), use_tls, normalized_host);
        if use_tls {
            if let Some(ca) = &self.upstream_ca {
                peer.options.ca = Some(ca.clone());
            }
            if let Some(alternative_cn) = alternative_cn {
                peer.options.alternative_cn = Some(alternative_cn);
            }
        }
        Ok(Box::new(peer))
    }

    async fn upstream_request_filter(
        &self,
        session: &mut Session,
        upstream_request: &mut RequestHeader,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        // 1. Add standard proxy headers
        #[allow(clippy::collapsible_if)]
        if let Some(addr) = session.client_addr() {
            if let Some(inet) = addr.as_inet() {
                let ip = inet.ip().to_string();
                upstream_request.insert_header("X-Forwarded-For", &ip)?;
                upstream_request.insert_header("X-Real-IP", &ip)?;
            }
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

        // 2. Propagate Trace Context
        let context = ctx.span.context();
        let mut injector = PingoraHeaderInjector(upstream_request);
        opentelemetry::global::get_text_map_propagator(|propagator| {
            propagator.inject_context(&context, &mut injector);
        });

        Ok(())
    }

    async fn response_filter(
        &self,
        _session: &mut Session,
        upstream_response: &mut ResponseHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<()> {
        // 1. Security Headers
        // HSTS - 1 year
        if upstream_response
            .headers
            .get("Strict-Transport-Security")
            .is_none()
        {
            upstream_response.insert_header(
                "Strict-Transport-Security",
                "max-age=31536000; includeSubDomains; preload",
            )?;
        }

        // X-Content-Type-Options
        if upstream_response
            .headers
            .get("X-Content-Type-Options")
            .is_none()
        {
            upstream_response.insert_header("X-Content-Type-Options", "nosniff")?;
        }

        // X-Frame-Options
        if upstream_response.headers.get("X-Frame-Options").is_none() {
            upstream_response.insert_header("X-Frame-Options", "SAMEORIGIN")?;
        }

        // Referrer-Policy
        if upstream_response.headers.get("Referrer-Policy").is_none() {
            upstream_response
                .insert_header("Referrer-Policy", "strict-origin-when-cross-origin")?;
        }

        Ok(())
    }

    async fn logging(&self, session: &mut Session, _e: Option<&Error>, ctx: &mut Self::CTX) {
        self.metrics.requests_total.fetch_add(1, Ordering::Relaxed);

        // Record latency
        let latency = chrono::Utc::now()
            .signed_duration_since(ctx.request_start_time)
            .num_milliseconds();
        self.metrics
            .latency_sum_ms
            .fetch_add(latency.max(0).cast_unsigned(), Ordering::Relaxed);

        if let Some(response) = session.response_written() {
            let code = response.status.as_u16();
            if (200..300).contains(&code) {
                self.metrics.responses_2xx.fetch_add(1, Ordering::Relaxed);
            } else if (300..400).contains(&code) {
                self.metrics.responses_3xx.fetch_add(1, Ordering::Relaxed);
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
