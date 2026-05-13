use crate::auth::extractor::extract_token_from_headers_and_uri;
use crate::auth::jwt;
use axum::{
    Json,
    body::Body,
    http::{HeaderValue, Method, Request, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use dashmap::DashMap;
use serde::Serialize;
use std::borrow::Cow;
use std::net::SocketAddr;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    pub public_rpm: u32,
    pub rate_limit_auth_login_rpm: u32,
    pub rate_limit_auth_register_rpm: u32,
    pub rate_limit_github_install_rpm: u32,
    pub rate_limit_apps_create_rpm: u32,
    pub rate_limit_apps_deploy_rpm: u32,
    pub rate_limit_webhooks_github_generic_rpm: u32,
    pub rate_limit_webhooks_github_named_rpm: u32,
    pub authenticated_read_rpm: u32,
    pub authenticated_write_rpm: u32,
    pub authenticated_stream_rpm: u32,
    pub entry_ttl: Duration,
    pub cleanup_every_requests: u64,
    pub trust_proxy_headers: bool,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self::from_profile(DeploymentEnv::Development)
    }
}

impl RateLimitConfig {
    fn from_profile(env: DeploymentEnv) -> Self {
        let profile = RateLimitProfile::for_env(env);
        Self {
            public_rpm: profile.public_rpm,
            rate_limit_auth_login_rpm: profile.rate_limit_auth_login_rpm,
            rate_limit_auth_register_rpm: profile.rate_limit_auth_register_rpm,
            rate_limit_github_install_rpm: profile.rate_limit_github_install_rpm,
            rate_limit_apps_create_rpm: profile.rate_limit_apps_create_rpm,
            rate_limit_apps_deploy_rpm: profile.rate_limit_apps_deploy_rpm,
            rate_limit_webhooks_github_generic_rpm: profile.rate_limit_webhooks_github_generic_rpm,
            rate_limit_webhooks_github_named_rpm: profile.rate_limit_webhooks_github_named_rpm,
            authenticated_read_rpm: profile.authenticated_read_rpm,
            authenticated_write_rpm: profile.authenticated_write_rpm,
            authenticated_stream_rpm: profile.authenticated_stream_rpm,
            entry_ttl: Duration::from_secs(15 * 60),
            cleanup_every_requests: 512,
            trust_proxy_headers: false,
        }
    }

    pub fn from_api_config(config: &crate::config::ApiConfig) -> anyhow::Result<Self> {
        let env = DeploymentEnv::parse(&config.deployment_env)?;
        let profile = RateLimitProfile::for_env(env);
        let value = Self {
            public_rpm: config.rate_limit_public_rpm.unwrap_or(profile.public_rpm),
            rate_limit_auth_login_rpm: config
                .rate_limit_auth_login_rpm
                .unwrap_or(profile.rate_limit_auth_login_rpm),
            rate_limit_auth_register_rpm: config
                .rate_limit_auth_register_rpm
                .unwrap_or(profile.rate_limit_auth_register_rpm),
            rate_limit_github_install_rpm: config
                .rate_limit_github_install_rpm
                .unwrap_or(profile.rate_limit_github_install_rpm),
            rate_limit_apps_create_rpm: config
                .rate_limit_apps_create_rpm
                .unwrap_or(profile.rate_limit_apps_create_rpm),
            rate_limit_apps_deploy_rpm: config
                .rate_limit_apps_deploy_rpm
                .unwrap_or(profile.rate_limit_apps_deploy_rpm),
            rate_limit_webhooks_github_generic_rpm: config
                .rate_limit_webhooks_github_generic_rpm
                .unwrap_or(profile.rate_limit_webhooks_github_generic_rpm),
            rate_limit_webhooks_github_named_rpm: config
                .rate_limit_webhooks_github_named_rpm
                .unwrap_or(profile.rate_limit_webhooks_github_named_rpm),
            authenticated_read_rpm: config
                .rate_limit_authenticated_read_rpm
                .unwrap_or(profile.authenticated_read_rpm),
            authenticated_write_rpm: config
                .rate_limit_authenticated_write_rpm
                .unwrap_or(profile.authenticated_write_rpm),
            authenticated_stream_rpm: config
                .rate_limit_authenticated_stream_rpm
                .unwrap_or(profile.authenticated_stream_rpm),
            entry_ttl: Duration::from_secs(config.rate_limit_entry_ttl_secs),
            cleanup_every_requests: config.rate_limit_cleanup_every_requests,
            trust_proxy_headers: config.rate_limit_trust_proxy_headers,
        };

        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.public_rpm > 0,
            "RATE_LIMIT_PUBLIC_RPM must be greater than 0"
        );
        anyhow::ensure!(
            self.rate_limit_auth_login_rpm > 0,
            "RATE_LIMIT_AUTH_LOGIN_RPM must be greater than 0"
        );
        anyhow::ensure!(
            self.rate_limit_auth_register_rpm > 0,
            "RATE_LIMIT_AUTH_REGISTER_RPM must be greater than 0"
        );
        anyhow::ensure!(
            self.rate_limit_github_install_rpm > 0,
            "RATE_LIMIT_GITHUB_INSTALL_RPM must be greater than 0"
        );
        anyhow::ensure!(
            self.rate_limit_apps_create_rpm > 0,
            "RATE_LIMIT_APPS_CREATE_RPM must be greater than 0"
        );
        anyhow::ensure!(
            self.rate_limit_apps_deploy_rpm > 0,
            "RATE_LIMIT_APPS_DEPLOY_RPM must be greater than 0"
        );
        anyhow::ensure!(
            self.rate_limit_webhooks_github_generic_rpm > 0,
            "RATE_LIMIT_WEBHOOKS_GITHUB_GENERIC_RPM must be greater than 0"
        );
        anyhow::ensure!(
            self.rate_limit_webhooks_github_named_rpm > 0,
            "RATE_LIMIT_WEBHOOKS_GITHUB_NAMED_RPM must be greater than 0"
        );
        anyhow::ensure!(
            self.authenticated_read_rpm > 0,
            "RATE_LIMIT_AUTHENTICATED_READ_RPM must be greater than 0"
        );
        anyhow::ensure!(
            self.authenticated_write_rpm > 0,
            "RATE_LIMIT_AUTHENTICATED_WRITE_RPM must be greater than 0"
        );
        anyhow::ensure!(
            self.authenticated_stream_rpm > 0,
            "RATE_LIMIT_AUTHENTICATED_STREAM_RPM must be greater than 0"
        );
        anyhow::ensure!(
            self.entry_ttl > Duration::from_secs(0),
            "RATE_LIMIT_ENTRY_TTL_SECS must be greater than 0"
        );
        anyhow::ensure!(
            self.cleanup_every_requests > 0,
            "RATE_LIMIT_CLEANUP_EVERY_REQUESTS must be greater than 0"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RateLimitClass {
    Public,
    AuthenticatedRead,
    AuthenticatedWrite,
    AuthenticatedStream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeploymentEnv {
    Development,
    Staging,
    Production,
}

impl DeploymentEnv {
    fn parse(value: &str) -> anyhow::Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "dev" | "development" | "local" => Ok(Self::Development),
            "staging" | "stage" => Ok(Self::Staging),
            "prod" | "production" => Ok(Self::Production),
            other => Err(anyhow::anyhow!(
                "Invalid DEPLOYMENT_ENV `{other}`. Expected development, staging, or production"
            )),
        }
    }
}

#[derive(Debug, Clone)]
struct RateLimitProfile {
    public_rpm: u32,
    rate_limit_auth_login_rpm: u32,
    rate_limit_auth_register_rpm: u32,
    rate_limit_github_install_rpm: u32,
    rate_limit_apps_create_rpm: u32,
    rate_limit_apps_deploy_rpm: u32,
    rate_limit_webhooks_github_generic_rpm: u32,
    rate_limit_webhooks_github_named_rpm: u32,
    authenticated_read_rpm: u32,
    authenticated_write_rpm: u32,
    authenticated_stream_rpm: u32,
}

impl RateLimitProfile {
    fn for_env(env: DeploymentEnv) -> Self {
        match env {
            DeploymentEnv::Development => Self {
                public_rpm: 300,
                rate_limit_auth_login_rpm: 120,
                rate_limit_auth_register_rpm: 60,
                rate_limit_github_install_rpm: 120,
                rate_limit_apps_create_rpm: 120,
                rate_limit_apps_deploy_rpm: 120,
                rate_limit_webhooks_github_generic_rpm: 240,
                rate_limit_webhooks_github_named_rpm: 240,
                authenticated_read_rpm: 1200,
                authenticated_write_rpm: 600,
                authenticated_stream_rpm: 120,
            },
            DeploymentEnv::Staging => Self {
                public_rpm: 120,
                rate_limit_auth_login_rpm: 20,
                rate_limit_auth_register_rpm: 20,
                rate_limit_github_install_rpm: 30,
                rate_limit_apps_create_rpm: 20,
                rate_limit_apps_deploy_rpm: 30,
                rate_limit_webhooks_github_generic_rpm: 30,
                rate_limit_webhooks_github_named_rpm: 60,
                authenticated_read_rpm: 600,
                authenticated_write_rpm: 240,
                authenticated_stream_rpm: 60,
            },
            DeploymentEnv::Production => Self {
                public_rpm: 60,
                rate_limit_auth_login_rpm: 10,
                rate_limit_auth_register_rpm: 10,
                rate_limit_github_install_rpm: 15,
                rate_limit_apps_create_rpm: 10,
                rate_limit_apps_deploy_rpm: 20,
                rate_limit_webhooks_github_generic_rpm: 20,
                rate_limit_webhooks_github_named_rpm: 30,
                authenticated_read_rpm: 300,
                authenticated_write_rpm: 120,
                authenticated_stream_rpm: 30,
            },
        }
    }
}

#[derive(Debug, Clone)]
struct RouteTarget {
    class: RateLimitClass,
    bucket: Cow<'static, str>,
}

#[derive(Debug)]
struct BucketPolicy {
    capacity: f64,
    refill_per_second: f64,
}

impl BucketPolicy {
    fn from_rpm(rpm: u32) -> Self {
        let capacity = rpm as f64;
        Self {
            capacity,
            refill_per_second: capacity / 60.0,
        }
    }
}

#[derive(Debug)]
struct BucketState {
    tokens: f64,
    last_refill: Instant,
    last_seen: Instant,
}

impl BucketState {
    fn new(now: Instant, policy: &BucketPolicy) -> Self {
        Self {
            tokens: policy.capacity,
            last_refill: now,
            last_seen: now,
        }
    }
}

#[derive(Debug)]
struct RateLimitOutcome {
    allowed: bool,
    retry_after: Duration,
    limit: u32,
    remaining: u32,
}

#[derive(Debug, Clone)]
enum RateLimitKey {
    PublicIp { bucket: String, ip: String },
    User { bucket: String, user_id: String },
}

impl RateLimitKey {
    fn as_string(&self) -> String {
        match self {
            Self::PublicIp { bucket, ip } => format!("public:{bucket}:ip:{ip}"),
            Self::User { bucket, user_id } => format!("auth:{bucket}:user:{user_id}"),
        }
    }
}

#[derive(Debug)]
pub struct RateLimiter {
    config: RateLimitConfig,
    jwt_secret: Arc<str>,
    entries: DashMap<String, Arc<std::sync::Mutex<BucketState>>>,
    request_counter: AtomicU64,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig, jwt_secret: String) -> anyhow::Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            jwt_secret: Arc::from(jwt_secret),
            entries: DashMap::new(),
            request_counter: AtomicU64::new(0),
        })
    }

    fn policy_for_class(&self, class: RateLimitClass) -> BucketPolicy {
        match class {
            RateLimitClass::Public => BucketPolicy::from_rpm(self.config.public_rpm),
            RateLimitClass::AuthenticatedRead => {
                BucketPolicy::from_rpm(self.config.authenticated_read_rpm)
            },
            RateLimitClass::AuthenticatedWrite => {
                BucketPolicy::from_rpm(self.config.authenticated_write_rpm)
            },
            RateLimitClass::AuthenticatedStream => {
                BucketPolicy::from_rpm(self.config.authenticated_stream_rpm)
            },
        }
    }

    fn policy_for_target(&self, target: &RouteTarget) -> BucketPolicy {
        if let Some(rpm) = self.route_specific_rpm(target.bucket.as_ref()) {
            return BucketPolicy::from_rpm(rpm);
        }

        self.policy_for_class(target.class)
    }

    fn classify_request(method: &Method, path: &str) -> Option<RouteTarget> {
        if method == Method::OPTIONS {
            return None;
        }

        let segments: Vec<&str> = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();
        if segments.len() < 2 || segments[0] != "v1" {
            return None;
        }

        match segments.as_slice() {
            ["v1", "auth", "register"] => Some(RouteTarget {
                class: RateLimitClass::Public,
                bucket: Cow::Borrowed("auth.register"),
            }),
            ["v1", "auth", "login"] => Some(RouteTarget {
                class: RateLimitClass::Public,
                bucket: Cow::Borrowed("auth.login"),
            }),
            ["v1", "github", "callback"] => Some(RouteTarget {
                class: RateLimitClass::Public,
                bucket: Cow::Borrowed("github.callback"),
            }),
            ["v1", "webhooks", "github"] => Some(RouteTarget {
                class: RateLimitClass::Public,
                bucket: Cow::Borrowed("webhooks.github.generic"),
            }),
            ["v1", "webhooks", "github", _app_name] => Some(RouteTarget {
                class: RateLimitClass::Public,
                bucket: Cow::Borrowed("webhooks.github.named"),
            }),
            ["v1", "github", "install"] => Some(RouteTarget {
                class: RateLimitClass::AuthenticatedRead,
                bucket: Cow::Borrowed("github.install"),
            }),
            ["v1", "github", "repos"] => Some(RouteTarget {
                class: RateLimitClass::AuthenticatedRead,
                bucket: Cow::Borrowed("github.repos"),
            }),
            ["v1", "github", "accounts"] => Some(RouteTarget {
                class: RateLimitClass::AuthenticatedRead,
                bucket: Cow::Borrowed("github.accounts"),
            }),
            ["v1", "auth", "me"] if method == Method::GET => Some(RouteTarget {
                class: RateLimitClass::AuthenticatedRead,
                bucket: Cow::Borrowed("auth.me.get"),
            }),
            ["v1", "auth", "me"] if method == Method::PUT => Some(RouteTarget {
                class: RateLimitClass::AuthenticatedWrite,
                bucket: Cow::Borrowed("auth.me.put"),
            }),
            ["v1", "apps"] if method == Method::GET => Some(RouteTarget {
                class: RateLimitClass::AuthenticatedRead,
                bucket: Cow::Borrowed("apps.list"),
            }),
            ["v1", "apps"] if method == Method::POST => Some(RouteTarget {
                class: RateLimitClass::AuthenticatedWrite,
                bucket: Cow::Borrowed("apps.create"),
            }),
            ["v1", "deploy"] if method == Method::POST => Some(RouteTarget {
                class: RateLimitClass::AuthenticatedWrite,
                bucket: Cow::Borrowed("deploy.global"),
            }),
            ["v1", "apps", _app_name] if method == Method::DELETE => Some(RouteTarget {
                class: RateLimitClass::AuthenticatedWrite,
                bucket: Cow::Borrowed("apps.delete"),
            }),
            ["v1", "apps", _app_name, "secret"] if method == Method::GET => Some(RouteTarget {
                class: RateLimitClass::AuthenticatedRead,
                bucket: Cow::Borrowed("apps.secret.get"),
            }),
            ["v1", "apps", _app_name, "deploy"] if method == Method::POST => Some(RouteTarget {
                class: RateLimitClass::AuthenticatedWrite,
                bucket: Cow::Borrowed("apps.deploy.trigger"),
            }),
            ["v1", "apps", _app_name, "deployments"] if method == Method::GET => {
                Some(RouteTarget {
                    class: RateLimitClass::AuthenticatedRead,
                    bucket: Cow::Borrowed("apps.deployments.list"),
                })
            },
            ["v1", "apps", _app_name, "deployments", "stream"] if method == Method::GET => {
                Some(RouteTarget {
                    class: RateLimitClass::AuthenticatedStream,
                    bucket: Cow::Borrowed("apps.deployments.stream"),
                })
            },
            ["v1", "apps", _app_name, "logs", "stream"] if method == Method::GET => {
                Some(RouteTarget {
                    class: RateLimitClass::AuthenticatedStream,
                    bucket: Cow::Borrowed("apps.logs.stream"),
                })
            },
            ["v1", "apps", _app_name, "metrics", "stream"] if method == Method::GET => {
                Some(RouteTarget {
                    class: RateLimitClass::AuthenticatedStream,
                    bucket: Cow::Borrowed("apps.metrics.stream"),
                })
            },
            [
                "v1",
                "apps",
                _app_name,
                "deployments",
                _deployment_id,
                "activate",
            ] if method == Method::POST => Some(RouteTarget {
                class: RateLimitClass::AuthenticatedWrite,
                bucket: Cow::Borrowed("apps.deployments.activate"),
            }),
            ["v1", "apps", _app_name, "deployments", _job_id] if method == Method::GET => {
                Some(RouteTarget {
                    class: RateLimitClass::AuthenticatedRead,
                    bucket: Cow::Borrowed("apps.deployments.status"),
                })
            },
            ["v1", "apps", _app_name, "deployments", _job_id] if method == Method::DELETE => {
                Some(RouteTarget {
                    class: RateLimitClass::AuthenticatedWrite,
                    bucket: Cow::Borrowed("apps.deployments.stop"),
                })
            },
            ["v1", "apps", _app_name, "deployments", _job_id, "logs"] if method == Method::GET => {
                Some(RouteTarget {
                    class: RateLimitClass::AuthenticatedStream,
                    bucket: Cow::Borrowed("apps.deployments.logs"),
                })
            },
            ["v1", "apps", _app_name, "deployments", _job_id, "pause"]
                if method == Method::POST =>
            {
                Some(RouteTarget {
                    class: RateLimitClass::AuthenticatedWrite,
                    bucket: Cow::Borrowed("apps.deployments.pause"),
                })
            },
            ["v1", "apps", _app_name, "deployments", _job_id, "resume"]
                if method == Method::POST =>
            {
                Some(RouteTarget {
                    class: RateLimitClass::AuthenticatedWrite,
                    bucket: Cow::Borrowed("apps.deployments.resume"),
                })
            },
            ["v1", "apps", _app_name, "deployments", _job_id, "delete"]
                if method == Method::DELETE =>
            {
                Some(RouteTarget {
                    class: RateLimitClass::AuthenticatedWrite,
                    bucket: Cow::Borrowed("apps.deployments.delete"),
                })
            },
            ["v1", "apps", _app_name, "security-groups"] if method == Method::GET => {
                Some(RouteTarget {
                    class: RateLimitClass::AuthenticatedRead,
                    bucket: Cow::Borrowed("apps.security_groups.list"),
                })
            },
            ["v1", "apps", _app_name, "security-groups"] if method == Method::POST => {
                Some(RouteTarget {
                    class: RateLimitClass::AuthenticatedWrite,
                    bucket: Cow::Borrowed("apps.security_groups.create"),
                })
            },
            ["v1", "apps", _app_name, "security-groups", _rule_id] if method == Method::DELETE => {
                Some(RouteTarget {
                    class: RateLimitClass::AuthenticatedWrite,
                    bucket: Cow::Borrowed("apps.security_groups.delete"),
                })
            },
            ["v1", "networking", "mesh"] if method == Method::GET => Some(RouteTarget {
                class: RateLimitClass::AuthenticatedRead,
                bucket: Cow::Borrowed("networking.mesh"),
            }),
            ["v1", "networking", "mesh", "stream"] if method == Method::GET => Some(RouteTarget {
                class: RateLimitClass::AuthenticatedStream,
                bucket: Cow::Borrowed("networking.mesh.stream"),
            }),
            ["v1", "deployments", "active"] if method == Method::GET => Some(RouteTarget {
                class: RateLimitClass::AuthenticatedRead,
                bucket: Cow::Borrowed("deployments.active"),
            }),
            ["v1", "deployments", "events"] if method == Method::GET => Some(RouteTarget {
                class: RateLimitClass::AuthenticatedStream,
                bucket: Cow::Borrowed("deployments.events"),
            }),
            ["v1", "workspace", "events"] if method == Method::GET => Some(RouteTarget {
                class: RateLimitClass::AuthenticatedStream,
                bucket: Cow::Borrowed("workspace.events"),
            }),
            _ if method == Method::GET || method == Method::HEAD => Some(RouteTarget {
                class: RateLimitClass::AuthenticatedRead,
                bucket: Cow::Owned(format!("get:{path}")),
            }),
            _ => Some(RouteTarget {
                class: RateLimitClass::AuthenticatedWrite,
                bucket: Cow::Owned(format!("{}:{path}", method.as_str().to_ascii_lowercase())),
            }),
        }
    }

    fn derive_key(&self, request: &Request<Body>, target: &RouteTarget) -> RateLimitKey {
        let bucket = target.bucket.to_string();
        match target.class {
            RateLimitClass::Public => RateLimitKey::PublicIp {
                bucket,
                ip: self.client_ip(request),
            },
            RateLimitClass::AuthenticatedRead
            | RateLimitClass::AuthenticatedWrite
            | RateLimitClass::AuthenticatedStream => {
                if let Ok(token) =
                    extract_token_from_headers_and_uri(request.headers(), request.uri())
                    && let Ok(claims) = jwt::verify_token(&token, &self.jwt_secret)
                {
                    return RateLimitKey::User {
                        bucket,
                        user_id: claims.sub,
                    };
                }

                RateLimitKey::PublicIp {
                    bucket,
                    ip: self.client_ip(request),
                }
            },
        }
    }

    fn route_specific_rpm(&self, bucket: &str) -> Option<u32> {
        match bucket {
            "auth.login" => Some(self.config.rate_limit_auth_login_rpm),
            "auth.register" => Some(self.config.rate_limit_auth_register_rpm),
            "github.install" => Some(self.config.rate_limit_github_install_rpm),
            "apps.create" => Some(self.config.rate_limit_apps_create_rpm),
            "apps.deploy.trigger" => Some(self.config.rate_limit_apps_deploy_rpm),
            "webhooks.github.generic" => Some(self.config.rate_limit_webhooks_github_generic_rpm),
            "webhooks.github.named" => Some(self.config.rate_limit_webhooks_github_named_rpm),
            _ => None,
        }
    }

    fn client_ip(&self, request: &Request<Body>) -> String {
        if self.config.trust_proxy_headers
            && let Some(ip) = forwarded_for_ip(request.headers())
        {
            return ip;
        }

        request
            .extensions()
            .get::<axum::extract::ConnectInfo<SocketAddr>>()
            .map(|ci| ci.0.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn get_bucket(&self, key: String, policy: &BucketPolicy) -> Arc<std::sync::Mutex<BucketState>> {
        self.entries
            .entry(key)
            .or_insert_with(|| {
                Arc::new(std::sync::Mutex::new(BucketState::new(
                    Instant::now(),
                    policy,
                )))
            })
            .clone()
    }

    fn cleanup_stale_entries(&self) {
        let now = Instant::now();
        let ttl = self.config.entry_ttl;
        self.entries.retain(|_, entry| {
            let guard = entry.lock().expect("rate limit bucket mutex poisoned");
            now.duration_since(guard.last_seen) <= ttl
        });
    }

    fn maybe_cleanup(&self) {
        let current = self.request_counter.fetch_add(1, Ordering::Relaxed) + 1;
        if current.is_multiple_of(self.config.cleanup_every_requests) {
            self.cleanup_stale_entries();
        }
    }

    fn check_bucket(&self, key: RateLimitKey, policy: &BucketPolicy) -> RateLimitOutcome {
        let bucket_key = key.as_string();
        let bucket = self.get_bucket(bucket_key, policy);
        let mut guard = bucket.lock().expect("rate limit bucket mutex poisoned");
        let now = Instant::now();
        let elapsed = now.duration_since(guard.last_refill).as_secs_f64();
        let refill = elapsed * policy.refill_per_second;
        if refill > 0.0 {
            guard.tokens = (guard.tokens + refill).min(policy.capacity);
            guard.last_refill = now;
        }
        guard.last_seen = now;

        if guard.tokens >= 1.0 {
            guard.tokens -= 1.0;
            let remaining = guard.tokens.floor() as u32;
            return RateLimitOutcome {
                allowed: true,
                retry_after: Duration::from_secs(0),
                limit: policy.capacity as u32,
                remaining,
            };
        }

        let missing = 1.0 - guard.tokens;
        let retry_secs = if policy.refill_per_second <= f64::EPSILON {
            1.0
        } else {
            (missing / policy.refill_per_second).ceil().max(1.0)
        };

        RateLimitOutcome {
            allowed: false,
            retry_after: Duration::from_secs(retry_secs as u64),
            limit: policy.capacity as u32,
            remaining: 0,
        }
    }

    fn evaluate(&self, request: &Request<Body>) -> Option<RateLimitOutcome> {
        let target = Self::classify_request(request.method(), request.uri().path())?;
        let policy = self.policy_for_target(&target);
        self.maybe_cleanup();
        let key = self.derive_key(request, &target);
        Some(self.check_bucket(key, &policy))
    }
}

#[derive(Debug, Serialize)]
struct RateLimitedBody {
    error: String,
    status: u16,
}

pub async fn rate_limit_middleware(
    axum::extract::State(rate_limiter): axum::extract::State<Arc<RateLimiter>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let outcome = rate_limiter.evaluate(&request);
    if let Some(outcome) = outcome.as_ref()
        && !outcome.allowed
    {
        let retry_after_secs = outcome.retry_after.as_secs().max(1);
        tracing::warn!(
            method = %request.method(),
            path = %request.uri().path(),
            retry_after = retry_after_secs,
            "Rate limit exceeded"
        );

        let mut response = (
            StatusCode::TOO_MANY_REQUESTS,
            Json(RateLimitedBody {
                error: "Too many requests".to_string(),
                status: StatusCode::TOO_MANY_REQUESTS.as_u16(),
            }),
        )
            .into_response();

        response.headers_mut().insert(
            header::RETRY_AFTER,
            HeaderValue::from_str(&retry_after_secs.to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("1")),
        );
        insert_rate_limit_headers(
            response.headers_mut(),
            outcome.limit,
            outcome.remaining,
            retry_after_secs,
        );
        return response;
    }

    let mut response = next.run(request).await;
    if let Some(outcome) = outcome {
        insert_rate_limit_headers(
            response.headers_mut(),
            outcome.limit,
            outcome.remaining,
            outcome.retry_after.as_secs().max(1),
        );
    }
    response
}

fn insert_rate_limit_headers(
    headers: &mut axum::http::HeaderMap,
    limit: u32,
    remaining: u32,
    retry_after_secs: u64,
) {
    let limit_value = HeaderValue::from_str(&limit.to_string());
    let remaining_value = HeaderValue::from_str(&remaining.to_string());
    let reset_value = HeaderValue::from_str(&retry_after_secs.to_string());

    if let Ok(value) = limit_value {
        headers.insert("x-ratelimit-limit", value);
    }
    if let Ok(value) = remaining_value {
        headers.insert("x-ratelimit-remaining", value);
    }
    if let Ok(value) = reset_value {
        headers.insert("x-ratelimit-reset", value);
    }
}

fn forwarded_for_ip(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{Request, StatusCode};

    #[test]
    fn public_paths_are_classified_and_bucketed_separately() {
        let register = RateLimiter::classify_request(&Method::POST, "/v1/auth/register").unwrap();
        let login = RateLimiter::classify_request(&Method::POST, "/v1/auth/login").unwrap();
        let webhook =
            RateLimiter::classify_request(&Method::POST, "/v1/webhooks/github/test").unwrap();

        assert_eq!(register.class, RateLimitClass::Public);
        assert_eq!(login.class, RateLimitClass::Public);
        assert_eq!(webhook.class, RateLimitClass::Public);
        assert_ne!(register.bucket, login.bucket);
        assert_ne!(login.bucket, webhook.bucket);
    }

    #[test]
    fn stream_paths_are_classified() {
        assert_eq!(
            RateLimiter::classify_request(&Method::GET, "/v1/apps/demo/logs/stream")
                .unwrap()
                .class,
            RateLimitClass::AuthenticatedStream
        );
        assert_eq!(
            RateLimiter::classify_request(&Method::GET, "/v1/apps/demo/deployments/123/logs")
                .unwrap()
                .class,
            RateLimitClass::AuthenticatedStream
        );
    }

    #[test]
    fn bucket_allows_then_rejects() {
        let limiter = RateLimiter::new(
            RateLimitConfig {
                public_rpm: 1,
                rate_limit_auth_login_rpm: 1,
                rate_limit_auth_register_rpm: 1,
                rate_limit_github_install_rpm: 1,
                rate_limit_apps_create_rpm: 1,
                rate_limit_apps_deploy_rpm: 1,
                rate_limit_webhooks_github_generic_rpm: 1,
                rate_limit_webhooks_github_named_rpm: 1,
                authenticated_read_rpm: 1,
                authenticated_write_rpm: 1,
                authenticated_stream_rpm: 1,
                entry_ttl: Duration::from_secs(60),
                cleanup_every_requests: 1,
                trust_proxy_headers: false,
            },
            "secret".to_string(),
        )
        .unwrap();

        let request = Request::builder()
            .method("POST")
            .uri("/v1/auth/login")
            .body(Body::empty())
            .unwrap();

        let first = limiter.evaluate(&request).unwrap();
        assert!(first.allowed);

        let second = limiter.evaluate(&request).unwrap();
        assert!(!second.allowed);
        assert_eq!(second.retry_after, Duration::from_secs(60));
    }

    #[test]
    fn distinct_routes_do_not_share_buckets() {
        let limiter = RateLimiter::new(
            RateLimitConfig {
                public_rpm: 1,
                rate_limit_auth_login_rpm: 1,
                rate_limit_auth_register_rpm: 1,
                rate_limit_github_install_rpm: 1,
                rate_limit_apps_create_rpm: 1,
                rate_limit_apps_deploy_rpm: 1,
                rate_limit_webhooks_github_generic_rpm: 1,
                rate_limit_webhooks_github_named_rpm: 1,
                authenticated_read_rpm: 1,
                authenticated_write_rpm: 1,
                authenticated_stream_rpm: 1,
                entry_ttl: Duration::from_secs(60),
                cleanup_every_requests: 1,
                trust_proxy_headers: false,
            },
            "secret".to_string(),
        )
        .unwrap();

        let login = Request::builder()
            .method("POST")
            .uri("/v1/auth/login")
            .body(Body::empty())
            .unwrap();
        let register = Request::builder()
            .method("POST")
            .uri("/v1/auth/register")
            .body(Body::empty())
            .unwrap();

        assert!(limiter.evaluate(&login).unwrap().allowed);
        assert!(limiter.evaluate(&register).unwrap().allowed);
    }

    #[test]
    fn route_specific_limits_override_class_defaults() {
        let limiter = RateLimiter::new(
            RateLimitConfig {
                public_rpm: 10,
                rate_limit_auth_login_rpm: 1,
                rate_limit_auth_register_rpm: 2,
                rate_limit_github_install_rpm: 10,
                rate_limit_apps_create_rpm: 10,
                rate_limit_apps_deploy_rpm: 10,
                rate_limit_webhooks_github_generic_rpm: 10,
                rate_limit_webhooks_github_named_rpm: 10,
                authenticated_read_rpm: 10,
                authenticated_write_rpm: 10,
                authenticated_stream_rpm: 10,
                entry_ttl: Duration::from_secs(60),
                cleanup_every_requests: 1,
                trust_proxy_headers: false,
            },
            "secret".to_string(),
        )
        .unwrap();

        let login = Request::builder()
            .method("POST")
            .uri("/v1/auth/login")
            .body(Body::empty())
            .unwrap();
        let register = Request::builder()
            .method("POST")
            .uri("/v1/auth/register")
            .body(Body::empty())
            .unwrap();

        assert!(limiter.evaluate(&login).unwrap().allowed);
        assert!(!limiter.evaluate(&login).unwrap().allowed);
        assert!(limiter.evaluate(&register).unwrap().allowed);
        assert!(limiter.evaluate(&register).unwrap().allowed);
        assert!(!limiter.evaluate(&register).unwrap().allowed);
    }

    #[test]
    fn stale_entries_are_cleaned_up() {
        let limiter = RateLimiter::new(
            RateLimitConfig {
                public_rpm: 1,
                rate_limit_auth_login_rpm: 1,
                rate_limit_auth_register_rpm: 1,
                rate_limit_github_install_rpm: 1,
                rate_limit_apps_create_rpm: 1,
                rate_limit_apps_deploy_rpm: 1,
                rate_limit_webhooks_github_generic_rpm: 1,
                rate_limit_webhooks_github_named_rpm: 1,
                authenticated_read_rpm: 1,
                authenticated_write_rpm: 1,
                authenticated_stream_rpm: 1,
                entry_ttl: Duration::from_millis(1),
                cleanup_every_requests: 1,
                trust_proxy_headers: false,
            },
            "secret".to_string(),
        )
        .unwrap();

        let request = Request::builder()
            .method("POST")
            .uri("/v1/auth/login")
            .body(Body::empty())
            .unwrap();

        let _ = limiter.evaluate(&request).unwrap();
        std::thread::sleep(Duration::from_millis(10));
        limiter.cleanup_stale_entries();
        assert_eq!(limiter.entries.len(), 0);
    }

    #[test]
    fn rate_limit_response_has_expected_status() {
        let response = (
            StatusCode::TOO_MANY_REQUESTS,
            Json(RateLimitedBody {
                error: "Too many requests".to_string(),
                status: 429,
            }),
        )
            .into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }
}
