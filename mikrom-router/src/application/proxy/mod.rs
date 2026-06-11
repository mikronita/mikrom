use crate::application::traffic::RouterTrafficPublisher;
use crate::domain::health::{self, RouterHealth};
use crate::domain::state::State;
use async_trait::async_trait;
use dashmap::DashMap;
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
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::{info, warn};
use tracing_opentelemetry::OpenTelemetrySpanExt;

mod host;
mod http;
use host::HostName;

pub struct RouterMetricsCounters {
    pub requests_total: AtomicU64,
    pub responses_2xx: AtomicU64,
    pub responses_3xx: AtomicU64,
    pub responses_4xx: AtomicU64,
    pub responses_5xx: AtomicU64,
    pub latency_sum_ms: AtomicU64,
    pub acme_hits: AtomicU64,
    pub acme_misses: AtomicU64,
    pub redirects: AtomicU64,
    pub rate_limited: AtomicU64,
    pub route_wait_timeouts: AtomicU64,
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
            acme_hits: AtomicU64::new(0),
            acme_misses: AtomicU64::new(0),
            redirects: AtomicU64::new(0),
            rate_limited: AtomicU64::new(0),
            route_wait_timeouts: AtomicU64::new(0),
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
    health: Arc<RouterHealth>,
    acme_staging: bool,
    default_site_host: Option<HostName>,
    default_site_redirect_url: Option<String>,
    api_host: HostName,
    api_upstream: Arc<LoadBalancer<RoundRobin>>,
    web_host: HostName,
    web_upstream: Arc<LoadBalancer<RoundRobin>>,
    upstream_ca: Option<Arc<Box<[X509]>>>,
    pub metrics: Arc<RouterMetricsCounters>,
    traffic_publisher: Option<Arc<RouterTrafficPublisher>>,
    rate_limiter: Rate,
    rps_limit: isize,
    timeouts: RouterTimeouts,
    wake_up_failures: DashMap<String, (u32, std::time::Instant)>,
}

fn parse_upstream_targets(label: &str, targets: &str) -> anyhow::Result<Vec<String>> {
    let parsed: Vec<String> = targets
        .split(',')
        .map(str::trim)
        .filter(|target| !target.is_empty())
        .map(ToOwned::to_owned)
        .collect();

    if parsed.is_empty() {
        return Err(anyhow::anyhow!("{label} upstream target list cannot be empty"));
    }

    Ok(parsed)
}

#[derive(Clone)]
pub struct RouterTimeouts {
    downstream_request: Duration,
    downstream_response: Duration,
    upstream_connect: Duration,
    upstream_read: Duration,
    upstream_write: Duration,
    upstream_idle: Duration,
    route_wait: Duration,
}

impl RouterTimeouts {
    #[must_use]
    pub fn from_config(config: &crate::app::config::RouterConfig) -> Self {
        Self {
            downstream_request: config.downstream_request_timeout(),
            downstream_response: config.downstream_response_timeout(),
            upstream_connect: config.upstream_connect_timeout(),
            upstream_read: config.upstream_read_timeout(),
            upstream_write: config.upstream_write_timeout(),
            upstream_idle: config.upstream_idle_timeout(),
            route_wait: config.route_wait_timeout(),
        }
    }
}

impl Default for RouterTimeouts {
    fn default() -> Self {
        Self {
            downstream_request: Duration::from_secs(10),
            downstream_response: Duration::from_secs(30),
            upstream_connect: Duration::from_secs(5),
            upstream_read: Duration::from_secs(30),
            upstream_write: Duration::from_secs(30),
            upstream_idle: Duration::from_mins(1),
            route_wait: Duration::from_secs(30),
        }
    }
}

pub struct MikromCtx {
    pub(crate) request_id: String,
    pub(crate) span: tracing::Span,
    pub(crate) request_start_time: chrono::DateTime<chrono::Utc>,
    pub(crate) host: Option<HostName>,
    pub(crate) normalized_host: Option<HostName>,
    pub(crate) upstream: Option<String>,
}

const MAX_REQUEST_HEADERS: usize = 128;
const MAX_REQUEST_HEADER_BYTES: usize = 16 * 1024;
const MAX_REQUEST_BODY_BYTES: u64 = 16 * 1024 * 1024;
pub(crate) const ROUTER_SERVER_NAME: &str = "mikrom-router";
const STRIPPED_UPSTREAM_HEADERS: &[&str] = &[
    "connection",
    "proxy-connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailers",
    "transfer-encoding",
    "upgrade",
    "x-forwarded-for",
    "x-forwarded-host",
    "x-forwarded-proto",
    "x-real-ip",
];

static REQUEST_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

fn header_size_from_pairs<I, N, V>(headers: I) -> usize
where
    I: IntoIterator<Item = (N, V)>,
    N: AsRef<str>,
    V: AsRef<[u8]>,
{
    headers
        .into_iter()
        .map(|(name, value)| name.as_ref().len() + value.as_ref().len())
        .sum()
}

fn request_content_length_from_value(value: Option<&str>) -> Option<u64> {
    value.and_then(|value| value.parse::<u64>().ok())
}

fn strip_untrusted_forwarding_headers(upstream_request: &mut RequestHeader) {
    for header in STRIPPED_UPSTREAM_HEADERS {
        upstream_request.remove_header(*header);
    }
}

pub(crate) fn set_router_server_header(response: &mut ResponseHeader) -> Result<()> {
    response.remove_header(&::http::header::SERVER);
    response.insert_header(::http::header::SERVER, ROUTER_SERVER_NAME)?;
    Ok(())
}

impl MikromProxy {
    #[must_use]
    #[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
    pub fn new(
        state: Arc<RwLock<State>>,
        health: Arc<RouterHealth>,
        acme_staging: bool,
        default_site_host: String,
        default_site_redirect_url: String,
        api_upstream_targets: String,
        web_upstream_targets: String,
        upstream_ca: Option<Arc<Box<[X509]>>>,
        metrics: Arc<RouterMetricsCounters>,
        traffic_publisher: Option<Arc<RouterTrafficPublisher>>,
        rps_limit: isize,
        timeouts: RouterTimeouts,
    ) -> Self {
        let default_site_host = default_site_host.trim();
        let default_site_redirect_url = default_site_redirect_url.trim();
        let api_targets = parse_upstream_targets("API", api_upstream_targets.as_str())
            .expect("API upstream target list must be non-empty");
        let api_upstream = Arc::new(
            LoadBalancer::<RoundRobin>::try_from_iter(api_targets.as_slice())
                .expect("API upstream targets must be valid"),
        );
        let web_targets = parse_upstream_targets("web", web_upstream_targets.as_str())
            .expect("Web upstream target list must be non-empty");
        let web_upstream = Arc::new(
            LoadBalancer::<RoundRobin>::try_from_iter(web_targets.as_slice())
                .expect("Web upstream targets must be valid"),
        );

        Self {
            state,
            health,
            acme_staging,
            default_site_host: (!default_site_host.is_empty())
                .then(|| HostName::parse(default_site_host)),
            default_site_redirect_url: (!default_site_redirect_url.is_empty())
                .then(|| default_site_redirect_url.to_string()),
            api_host: HostName::parse("api.mikrom.spluca.org"),
            api_upstream,
            web_host: HostName::parse("mikrom.spluca.org"),
            web_upstream,
            upstream_ca,
            metrics,
            traffic_publisher,
            rate_limiter: Rate::new(Duration::from_secs(1)),
            rps_limit,
            timeouts,
            wake_up_failures: DashMap::new(),
        }
    }

    pub async fn get_lb_and_tls(
        &self,
        host: &str,
    ) -> Result<(Arc<LoadBalancer<RoundRobin>>, bool, Option<String>)> {
        let raw_host = host;
        let host = HostName::parse(raw_host);
        if host.as_str() == self.api_host.as_str() {
            return Ok((self.api_upstream.clone(), false, None));
        }
        if host.as_str() == self.web_host.as_str() {
            return Ok((self.web_upstream.clone(), false, None));
        }
        let state = self.state.read().await;

        let res = state
            .routes
            .get(host.as_str())
            .or_else(|| state.routes.get(raw_host));
        let res = res.map_or_else(
            || {
                Err(Error::explain(
                    ErrorType::HTTPStatus(404),
                    format!("No route found for host: {}", host.as_str()),
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

    pub async fn has_route(&self, host: &str) -> bool {
        let raw_host = host;
        let host = HostName::parse(raw_host);
        if host.as_str() == self.api_host.as_str() {
            return true;
        }
        if host.as_str() == self.web_host.as_str() {
            return true;
        }
        let state = self.state.read().await;
        state.routes.contains_key(host.as_str()) || state.routes.contains_key(raw_host)
    }

    fn request_header_size(session: &Session) -> usize {
        header_size_from_pairs(
            session
                .req_header()
                .headers
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_bytes())),
        )
    }

    fn request_body_too_large(session: &Session) -> bool {
        request_content_length_from_value(
            session
                .req_header()
                .headers
                .get("content-length")
                .and_then(|value| value.to_str().ok()),
        )
        .is_some_and(|content_length| content_length > MAX_REQUEST_BODY_BYTES)
    }

    async fn enforce_request_limits(&self, session: &mut Session) -> Result<Option<bool>> {
        if session.req_header().headers.len() > MAX_REQUEST_HEADERS
            || Self::request_header_size(session) > MAX_REQUEST_HEADER_BYTES
        {
            return http::write_text_response(
                session,
                431,
                &[("Content-Type", "text/plain")],
                "Request Header Fields Too Large\n",
                false,
            )
            .await
            .map(Some);
        }

        if Self::request_body_too_large(session) {
            return http::write_text_response(
                session,
                413,
                &[("Content-Type", "text/plain")],
                "Request body too large\n",
                false,
            )
            .await
            .map(Some);
        }

        Ok(None)
    }

    fn apply_downstream_timeouts(&self, session: &mut Session) {
        let downstream = session.as_downstream_mut();
        downstream.set_read_timeout(Some(self.timeouts.downstream_request));
        downstream.set_write_timeout(Some(self.timeouts.downstream_response));
    }

    fn is_default_site_host(&self, host: &str) -> bool {
        self.default_site_host
            .as_ref()
            .is_some_and(|default_host| default_host.as_str() == HostName::parse(host).as_str())
    }

    fn default_site_redirect_location(&self, path_and_query: &str) -> Option<String> {
        let target = self.default_site_redirect_url.as_deref()?;
        Some(format!(
            "{}{}",
            target,
            path_and_query.trim_start_matches('/')
        ))
    }

    async fn wait_for_route(
        &self,
        host: &str,
        normalized_host: &str,
    ) -> Result<(Arc<LoadBalancer<RoundRobin>>, bool, Option<String>)> {
        let deadline = std::time::Instant::now() + self.timeouts.route_wait;

        loop {
            match self.get_lb_and_tls(host).await {
                Ok(route) => return Ok(route),
                Err(e) => {
                    if self.has_route(host).await {
                        return Err(e);
                    }

                    if std::time::Instant::now() >= deadline {
                        return Err(Error::explain(
                            ErrorType::HTTPStatus(503),
                            format!("Route for host {normalized_host} is starting up"),
                        ));
                    }

                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                },
            }
        }
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

        let key_auth = self.lookup_acme_key_auth(token).await;

        if let Some(key_auth) = key_auth {
            self.metrics.acme_hits.fetch_add(1, Ordering::Relaxed);
            if self.acme_staging {
                tracing::info!(
                    "ACME challenge received for host {host}: token={token}, responding with key_auth"
                );
            }

            let result = http::write_text_response(
                session,
                200,
                &[("Content-Type", "text/plain")],
                &key_auth,
                false,
            )
            .await?;
            return Ok(Some(result));
        }

        self.metrics.acme_misses.fetch_add(1, Ordering::Relaxed);
        if self.acme_staging {
            tracing::warn!(
                "ACME challenge received for host {host} but token {token} not found in state"
            );
        }

        Ok(None)
    }

    async fn lookup_acme_key_auth(&self, token: &str) -> Option<String> {
        const ACME_LOOKUP_RETRIES: usize = 10;
        const ACME_LOOKUP_DELAY: Duration = Duration::from_millis(200);

        for attempt in 0..=ACME_LOOKUP_RETRIES {
            let key_auth = {
                let state = self.state.read().await;
                state.acme_tokens.get(token).cloned()
            };

            if key_auth.is_some() || attempt == ACME_LOOKUP_RETRIES {
                return key_auth;
            }

            tokio::time::sleep(ACME_LOOKUP_DELAY).await;
        }

        None
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
        set_router_server_header(&mut redirect)?;
        redirect.insert_header("Location", location)?;
        session
            .write_response_header(Box::new(redirect), true)
            .await?;
        self.metrics.redirects.fetch_add(1, Ordering::Relaxed);
        Ok(Some(true))
    }

    async fn maybe_redirect_default_site(
        &self,
        session: &mut Session,
        host: &str,
        path_and_query: &str,
    ) -> Result<Option<bool>> {
        if !self.is_default_site_host(host) {
            return Ok(None);
        }

        let Some(location) = self.default_site_redirect_location(path_and_query) else {
            return Ok(None);
        };

        let mut redirect = ResponseHeader::build(307, None)?;
        set_router_server_header(&mut redirect)?;
        redirect.insert_header("Location", location)?;
        redirect.insert_header("Cache-Control", "no-store")?;
        session
            .write_response_header(Box::new(redirect), true)
            .await?;
        self.metrics.redirects.fetch_add(1, Ordering::Relaxed);
        Ok(Some(true))
    }

    async fn maybe_handle_health_endpoint(
        &self,
        session: &mut Session,
        path: &str,
    ) -> Result<Option<bool>> {
        match path {
            "/health/live" => health::write_text_response(session, 200, "alive\n")
                .await
                .map(Some),
            "/health/ready" => {
                let snapshot = self.health.snapshot();
                let status = if snapshot.ready { 200 } else { 503 };
                health::write_snapshot_response(session, status, &snapshot)
                    .await
                    .map(Some)
            },
            "/health/deps" => {
                let snapshot = self.health.snapshot();
                let status = if snapshot.dependencies_ready {
                    200
                } else {
                    503
                };
                health::write_snapshot_response(session, status, &snapshot)
                    .await
                    .map(Some)
            },
            "/health/control-plane" => {
                let snapshot = self.health.snapshot();
                let status = if snapshot.control_plane_synced {
                    200
                } else {
                    503
                };
                health::write_snapshot_response(session, status, &snapshot)
                    .await
                    .map(Some)
            },
            _ => Ok(None),
        }
    }
}

struct PingoraHeaderInjector<'a>(&'a mut RequestHeader);

impl Injector for PingoraHeaderInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        use ::http::header::HeaderName;
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
            .map(::http::HeaderName::as_str)
            .collect()
    }
}

#[async_trait]
impl ProxyHttp for MikromProxy {
    type CTX = MikromCtx;
    fn new_ctx(&self) -> Self::CTX {
        let request_seq = REQUEST_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        let request_time = chrono::Utc::now();
        MikromCtx {
            request_id: format!(
                "{}-{:x}",
                request_time.timestamp_nanos_opt().unwrap_or_default(),
                request_seq
            ),
            span: tracing::Span::none(),
            request_start_time: request_time,
            host: None,
            normalized_host: None,
            upstream: None,
        }
    }

    fn init_downstream_modules(&self, modules: &mut HttpModules) {
        modules.add_module(ResponseCompressionBuilder::enable(3));
    }

    async fn request_filter(&self, session: &mut Session, ctx: &mut Self::CTX) -> Result<bool> {
        // Extract tracing context and start the request span.
        let parent_cx = opentelemetry::global::get_text_map_propagator(|propagator| {
            propagator.extract(&PingoraHeaderExtractor(session.req_header()))
        });

        let span = tracing::info_span!("proxy_request", 
            method = ?session.req_header().method,
            uri = %session.req_header().uri);
        let _ = span.set_parent(parent_cx);
        ctx.span = span;

        self.apply_downstream_timeouts(session);
        let request_path = session.req_header().uri.path().to_string();

        if let Some(result) = self
            .maybe_handle_health_endpoint(session, request_path.as_str())
            .await?
        {
            return Ok(result);
        }

        // Apply per-IP rate limiting.
        if let Some(addr) = session.client_addr() {
            let ip = addr.to_string();
            let curr_window_requests = self.rate_limiter.observe(&ip, 1);
            if curr_window_requests > self.rps_limit {
                warn!("Rate limit exceeded for IP: {ip} (requests: {curr_window_requests})");
                self.metrics.rate_limited.fetch_add(1, Ordering::Relaxed);
                return http::write_text_response(
                    session,
                    429,
                    &[("Content-Type", "text/plain"), ("Retry-After", "1")],
                    "Too Many Requests\n",
                    false,
                )
                .await;
            }
        }

        if let Some(result) = self.enforce_request_limits(session).await? {
            return Ok(result);
        }

        let host = session
            .get_header("Host")
            .and_then(|h| h.to_str().ok())
            .or_else(|| session.req_header().uri.host())
            .unwrap_or("")
            .to_string();
        let normalized_host = HostName::parse(&host);

        let path = session.req_header().uri.path().to_string();
        let path_and_query = session
            .req_header()
            .uri
            .path_and_query()
            .map_or_else(|| path.clone(), |value| value.as_str().to_string());
        ctx.host = Some(HostName::parse(&host));
        ctx.normalized_host = Some(normalized_host.clone());

        if !normalized_host.as_str().is_empty()
            && let Some(publisher) = &self.traffic_publisher
        {
            publisher.record(normalized_host.as_str().to_string());
        }

        if self
            .maybe_handle_acme_challenge(session, &host, &path)
            .await?
            == Some(true)
        {
            return Ok(true);
        }

        if self
            .maybe_redirect_default_site(session, normalized_host.as_str(), path_and_query.as_str())
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
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        let host = session
            .get_header("Host")
            .and_then(|h| h.to_str().ok())
            .or_else(|| session.req_header().uri.host())
            .unwrap_or("");
        let normalized_host = HostName::parse(host);

        // Check the circuit breaker.
        if let Some(entry) = self.wake_up_failures.get(normalized_host.as_str()) {
            let (count, last_failure) = *entry;
            if count >= 3 && last_failure.elapsed() < Duration::from_mins(1) {
                return Err(Error::explain(
                    ErrorType::HTTPStatus(503),
                    format!(
                        "Application {} is currently unavailable (circuit breaker open)",
                        normalized_host.as_str()
                    ),
                ));
            }
        }

        let start_time = std::time::Instant::now();
        let deadline = start_time + self.timeouts.route_wait;
        let mut last_log_time = start_time;

        loop {
            // Publish a traffic event to wake up the app if needed.
            if let Some(publisher) = &self.traffic_publisher {
                publisher.record(normalized_host.as_str().to_string());
            }

            let (lb, use_tls, alternative_cn) = if self.has_route(host).await {
                self.get_lb_and_tls(host).await?
            } else {
                self.wait_for_route(host, normalized_host.as_str()).await?
            };

            // Use the client address as the load-balancer hash seed when available.
            let hash = session
                .client_addr()
                .map_or_else(|| b"".to_vec(), |addr| addr.to_string().into_bytes());

            if let Some(upstream) = lb.select(&hash, 256) {
                let addr_str = upstream.addr.to_string();
                ctx.upstream = Some(addr_str.clone());
                info!(
                    request_id = %ctx.request_id,
                    host = %normalized_host.as_str(),
                    upstream = %addr_str,
                    use_tls,
                    "Selected upstream"
                );

                // Reset the circuit breaker on success.
                self.wake_up_failures.remove(normalized_host.as_str());

                let mut peer =
                    HttpPeer::new(addr_str, use_tls, normalized_host.as_str().to_string());
                peer.options.connection_timeout = Some(self.timeouts.upstream_connect);
                peer.options.total_connection_timeout = Some(self.timeouts.upstream_connect);
                peer.options.read_timeout = Some(self.timeouts.upstream_read);
                peer.options.write_timeout = Some(self.timeouts.upstream_write);
                peer.options.idle_timeout = Some(self.timeouts.upstream_idle);
                if use_tls {
                    if let Some(ca) = &self.upstream_ca {
                        peer.options.ca = Some(ca.clone());
                    }
                    if let Some(alternative_cn) = &alternative_cn {
                        peer.options.alternative_cn = Some(alternative_cn.clone());
                    }
                }
                return Ok(Box::new(peer));
            }

            let now = std::time::Instant::now();
            if now >= deadline {
                self.metrics
                    .route_wait_timeouts
                    .fetch_add(1, Ordering::Relaxed);
                return Err(Error::explain(
                    ErrorType::HTTPStatus(503),
                    format!(
                        "No healthy upstreams for host: {} after waiting 30s",
                        normalized_host.as_str()
                    ),
                ));
            }

            // Log selection failures every 2 seconds to avoid spamming while keeping visibility.
            if now.duration_since(last_log_time).as_secs() >= 2 {
                info!(
                    "No healthy upstreams for {} yet (app might be waking up), waiting...",
                    normalized_host.as_str()
                );
                last_log_time = now;
            }

            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    }

    fn fail_to_connect(
        &self,
        session: &mut Session,
        _peer: &HttpPeer,
        ctx: &mut Self::CTX,
        e: Box<Error>,
    ) -> Box<Error> {
        let host = ctx
            .normalized_host
            .as_ref()
            .map(|host| host.as_str().to_string())
            .or_else(|| {
                session
                    .get_header("Host")
                    .and_then(|h| h.to_str().ok())
                    .map(|host| HostName::parse(host).as_str().to_string())
            })
            .unwrap_or_default();

        if !host.is_empty() {
            let now = Instant::now();
            self.wake_up_failures
                .entry(host)
                .and_modify(|entry| {
                    entry.0 = entry.0.saturating_add(1);
                    entry.1 = now;
                })
                .or_insert((1, now));
        }

        let mut retry_e = e;
        retry_e.set_retry(true);
        retry_e
    }

    async fn fail_to_proxy(
        &self,
        session: &mut Session,
        e: &Error,
        _ctx: &mut Self::CTX,
    ) -> pingora::proxy::FailToProxy
    where
        Self::CTX: Send + Sync,
    {
        let code = match e.etype() {
            ErrorType::HTTPStatus(code) => *code,
            _ => match e.esource() {
                ErrorSource::Upstream => 502,
                ErrorSource::Downstream => match e.etype() {
                    ErrorType::WriteError | ErrorType::ReadError | ErrorType::ConnectionClosed => 0,
                    _ => 400,
                },
                ErrorSource::Internal | ErrorSource::Unset => 500,
            },
        };

        if code > 0 {
            let mut response = match ResponseHeader::build(code, Some(0)) {
                Ok(response) => response,
                Err(err) => {
                    warn!("Failed to build router error response: {err}");
                    ResponseHeader::build(500, Some(0))
                        .expect("failed to build fallback error response")
                },
            };
            if let Err(err) = set_router_server_header(&mut response) {
                warn!("Failed to set router server header on error response: {err}");
            }
            if let Err(err) =
                response.insert_header(::http::header::CACHE_CONTROL, "private, no-store")
            {
                warn!("Failed to set cache-control on error response: {err}");
            }
            if let Err(err) = response.insert_header(::http::header::CONTENT_LENGTH, "0") {
                warn!("Failed to set content-length on error response: {err}");
            }

            if let Err(err) = session
                .as_mut()
                .write_response_header(Box::new(response))
                .await
            {
                warn!("failed to send error response to downstream: {err}");
            }
        }

        pingora::proxy::FailToProxy {
            error_code: code,
            can_reuse_downstream: false,
        }
    }

    async fn upstream_request_filter(
        &self,
        session: &mut Session,
        upstream_request: &mut RequestHeader,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        // Add standard proxy headers.
        strip_untrusted_forwarding_headers(upstream_request);

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

        // Propagate trace context.
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
        set_router_server_header(upstream_response)?;

        // Add security headers.
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

        if upstream_response
            .headers
            .get("X-Content-Type-Options")
            .is_none()
        {
            upstream_response.insert_header("X-Content-Type-Options", "nosniff")?;
        }

        if upstream_response.headers.get("X-Frame-Options").is_none() {
            upstream_response.insert_header("X-Frame-Options", "SAMEORIGIN")?;
        }

        if upstream_response.headers.get("Referrer-Policy").is_none() {
            upstream_response
                .insert_header("Referrer-Policy", "strict-origin-when-cross-origin")?;
        }

        Ok(())
    }

    async fn logging(&self, session: &mut Session, _e: Option<&Error>, ctx: &mut Self::CTX) {
        self.metrics.requests_total.fetch_add(1, Ordering::Relaxed);

        // Record latency.
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
            info!(
                request_id = %ctx.request_id,
                host = %ctx
                    .normalized_host
                    .as_ref()
                    .map_or("unknown", HostName::as_str),
                upstream = %ctx.upstream.as_deref().unwrap_or("unknown"),
                status = code,
                latency_ms = latency,
                requests_total = self.metrics.requests_total.load(Ordering::Relaxed),
                responses_2xx = self.metrics.responses_2xx.load(Ordering::Relaxed),
                responses_3xx = self.metrics.responses_3xx.load(Ordering::Relaxed),
                responses_4xx = self.metrics.responses_4xx.load(Ordering::Relaxed),
                responses_5xx = self.metrics.responses_5xx.load(Ordering::Relaxed),
                acme_hits = self.metrics.acme_hits.load(Ordering::Relaxed),
                acme_misses = self.metrics.acme_misses.load(Ordering::Relaxed),
                redirects = self.metrics.redirects.load(Ordering::Relaxed),
                rate_limited = self.metrics.rate_limited.load(Ordering::Relaxed),
                route_wait_timeouts = self.metrics.route_wait_timeouts.load(Ordering::Relaxed),
                "Proxy request completed"
            );
        } else {
            self.metrics.responses_5xx.fetch_add(1, Ordering::Relaxed);
            info!(
                request_id = %ctx.request_id,
                host = %ctx
                    .normalized_host
                    .as_ref()
                    .map_or("unknown", HostName::as_str),
                upstream = %ctx.upstream.as_deref().unwrap_or("unknown"),
                status = 500_u16,
                latency_ms = latency,
                requests_total = self.metrics.requests_total.load(Ordering::Relaxed),
                responses_2xx = self.metrics.responses_2xx.load(Ordering::Relaxed),
                responses_3xx = self.metrics.responses_3xx.load(Ordering::Relaxed),
                responses_4xx = self.metrics.responses_4xx.load(Ordering::Relaxed),
                responses_5xx = self.metrics.responses_5xx.load(Ordering::Relaxed),
                acme_hits = self.metrics.acme_hits.load(Ordering::Relaxed),
                acme_misses = self.metrics.acme_misses.load(Ordering::Relaxed),
                redirects = self.metrics.redirects.load(Ordering::Relaxed),
                rate_limited = self.metrics.rate_limited.load(Ordering::Relaxed),
                route_wait_timeouts = self.metrics.route_wait_timeouts.load(Ordering::Relaxed),
                "Proxy request completed without response"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::health::RouterHealth;
    use crate::domain::state::{Route, State};
    use pingora::lb::LoadBalancer;
    use pingora::lb::selection::RoundRobin;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn test_has_route_detects_inserted_host() {
        let mut routes = HashMap::new();
        let targets = vec!["127.0.0.1:8080".to_string()];
        let lb = LoadBalancer::<RoundRobin>::try_from_iter(targets.as_slice()).unwrap();
        routes.insert(
            "app.example.com".to_string(),
            Route {
                host: "app.example.com".to_string(),
                targets,
                lb: Arc::new(lb),
                use_tls: false,
                tls_alternative_cn: None,
            },
        );

        let state = Arc::new(RwLock::new(State {
            routes,
            acme_tokens: HashMap::new(),
            certificates: HashMap::new(),
        }));

        let proxy = MikromProxy::new(
            state,
            Arc::new(RouterHealth::new()),
            false,
            String::new(),
            String::new(),
            "127.0.0.1:5001,[::1]:5001".to_string(),
            "127.0.0.1:5173,[::1]:5173".to_string(),
            None,
            Arc::new(RouterMetricsCounters::new()),
            None,
            100,
            RouterTimeouts::default(),
        );

        assert!(proxy.has_route("app.example.com").await);
        assert!(proxy.has_route("app.example.com:443").await);
        assert!(!proxy.has_route("missing.example.com").await);
    }

    #[tokio::test]
    async fn test_wait_for_route_detects_late_route() {
        let state = Arc::new(RwLock::new(State::default()));
        let proxy = MikromProxy::new(
            state.clone(),
            Arc::new(RouterHealth::new()),
            false,
            String::new(),
            String::new(),
            "127.0.0.1:5001,[::1]:5001".to_string(),
            "127.0.0.1:5173,[::1]:5173".to_string(),
            None,
            Arc::new(RouterMetricsCounters::new()),
            None,
            100,
            RouterTimeouts::default(),
        );

        let host = "late.example.com".to_string();
        let state_clone = state.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            let targets = vec!["127.0.0.1:8080".to_string()];
            let lb = LoadBalancer::<RoundRobin>::try_from_iter(targets.as_slice()).unwrap();
            let mut guard = state_clone.write().await;
            guard.routes.insert(
                host.clone(),
                Route {
                    host,
                    targets,
                    lb: Arc::new(lb),
                    use_tls: false,
                    tls_alternative_cn: None,
                },
            );
        });

        let result = proxy
            .wait_for_route("late.example.com", "late.example.com")
            .await
            .unwrap();
        assert!(!result.1);
        assert!(result.2.is_none());
    }

    #[test]
    fn test_default_site_redirect_location_preserves_path_and_query() {
        let proxy = MikromProxy::new(
            Arc::new(RwLock::new(State::default())),
            Arc::new(RouterHealth::new()),
            false,
            "debaser.spluca.org".to_string(),
            "https://spluca.org/".to_string(),
            "127.0.0.1:5001,[::1]:5001".to_string(),
            "127.0.0.1:5173,[::1]:5173".to_string(),
            None,
            Arc::new(RouterMetricsCounters::new()),
            None,
            100,
            RouterTimeouts::default(),
        );

        assert_eq!(
            proxy
                .default_site_redirect_location("/foo?bar=baz")
                .as_deref(),
            Some("https://spluca.org/foo?bar=baz")
        );
    }

    #[test]
    fn test_header_size_from_pairs_counts_names_and_values() {
        let size = header_size_from_pairs([("host", "app.example.com"), ("content-length", "123")]);

        assert_eq!(
            size,
            "host".len() + "app.example.com".len() + "content-length".len() + "123".len()
        );
    }

    #[test]
    fn test_request_content_length_parser_handles_invalid_values() {
        assert_eq!(request_content_length_from_value(Some("42")), Some(42));
        assert_eq!(
            request_content_length_from_value(Some("not-a-number")),
            None
        );
        assert_eq!(request_content_length_from_value(None), None);
    }

    #[test]
    fn test_set_router_server_header_overwrites_existing_value() {
        let mut response = ResponseHeader::build(200, Some(0)).unwrap();
        response.insert_header("Server", "Pingora").unwrap();

        set_router_server_header(&mut response).unwrap();

        assert_eq!(response.headers.get("server").unwrap(), ROUTER_SERVER_NAME);
    }

    #[test]
    fn test_router_timeouts_default_values() {
        let timeouts = RouterTimeouts::default();
        assert_eq!(timeouts.downstream_request, Duration::from_secs(10));
        assert_eq!(timeouts.downstream_response, Duration::from_secs(30));
        assert_eq!(timeouts.upstream_connect, Duration::from_secs(5));
        assert_eq!(timeouts.upstream_read, Duration::from_secs(30));
        assert_eq!(timeouts.upstream_write, Duration::from_secs(30));
        assert_eq!(timeouts.upstream_idle, Duration::from_mins(1));
        assert_eq!(timeouts.route_wait, Duration::from_secs(30));
    }

    #[test]
    fn test_strip_untrusted_forwarding_headers_removes_spoofed_values() {
        let mut header = RequestHeader::build("GET", b"/", None).unwrap();
        header.insert_header("Connection", "keep-alive").unwrap();
        header.insert_header("X-Forwarded-For", "1.2.3.4").unwrap();
        header.insert_header("X-Real-IP", "1.2.3.4").unwrap();
        header.insert_header("X-Forwarded-Proto", "https").unwrap();
        header.insert_header("X-Custom", "value").unwrap();

        strip_untrusted_forwarding_headers(&mut header);

        assert!(header.headers.get("connection").is_none());
        assert!(header.headers.get("x-forwarded-for").is_none());
        assert!(header.headers.get("x-real-ip").is_none());
        assert!(header.headers.get("x-forwarded-proto").is_none());
        assert_eq!(header.headers.get("x-custom").unwrap(), "value");
    }
}
