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
use std::net::SocketAddr;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
    pub cleanup_interval: Duration,
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
            cleanup_interval: Duration::from_secs(60),
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
            cleanup_interval: Duration::from_secs(config.rate_limit_cleanup_interval_secs),
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
            self.cleanup_interval > Duration::from_secs(0),
            "RATE_LIMIT_CLEANUP_INTERVAL_SECS must be greater than 0"
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
    bucket: &'static str,
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
    reset_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum RateLimitIdentity {
    SocketAddr(SocketAddr),
    ForwardedIp(String),
    UserId(String),
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RateLimitKey {
    bucket: &'static str,
    identity: RateLimitIdentity,
}

#[derive(Debug)]
pub struct RateLimiter {
    config: RateLimitConfig,
    jwt_secret: Arc<str>,
    entries: DashMap<RateLimitKey, Arc<std::sync::Mutex<BucketState>>>,
    cleanup_task_started: AtomicBool,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig, jwt_secret: String) -> anyhow::Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            jwt_secret: Arc::from(jwt_secret),
            entries: DashMap::new(),
            cleanup_task_started: AtomicBool::new(false),
        })
    }

    pub fn start_cleanup_task(self: &Arc<Self>) {
        if self.cleanup_task_started.swap(true, Ordering::AcqRel) {
            return;
        }

        let limiter = Arc::clone(self);
        tokio::spawn(async move {
            let interval = limiter.cleanup_interval();
            let mut ticker = tokio::time::interval(interval);
            ticker.tick().await;

            loop {
                ticker.tick().await;
                limiter.cleanup_stale_entries();
            }
        });
    }

    fn cleanup_interval(&self) -> Duration {
        let half_ttl = self.config.entry_ttl.div_f64(2.0);
        half_ttl.clamp(Duration::from_secs(1), Duration::from_secs(60))
    }

    fn split_path_segments(path: &str) -> impl Iterator<Item = &str> {
        path.split('/').filter(|segment| !segment.is_empty())
    }

    fn path_matches(path: &str, expected: &[&str]) -> bool {
        Self::split_path_segments(path).eq(expected.iter().copied())
    }

    fn path_has_prefix(path: &str, expected: &[&str]) -> bool {
        let mut segments = Self::split_path_segments(path);
        expected
            .iter()
            .copied()
            .all(|segment| segments.next() == Some(segment))
    }

    fn path_segment_count(path: &str) -> usize {
        Self::split_path_segments(path).count()
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

        if !Self::path_has_prefix(path, &["v1"]) {
            return None;
        }

        if Self::path_matches(path, &["v1", "auth", "register"]) {
            return Some(RouteTarget {
                class: RateLimitClass::Public,
                bucket: "auth.register",
            });
        }

        if Self::path_matches(path, &["v1", "auth", "login"]) {
            return Some(RouteTarget {
                class: RateLimitClass::Public,
                bucket: "auth.login",
            });
        }

        if Self::path_matches(path, &["v1", "github", "callback"]) {
            return Some(RouteTarget {
                class: RateLimitClass::Public,
                bucket: "github.callback",
            });
        }

        if Self::path_matches(path, &["v1", "webhooks", "github"]) {
            return Some(RouteTarget {
                class: RateLimitClass::Public,
                bucket: "webhooks.github.generic",
            });
        }

        if Self::path_has_prefix(path, &["v1", "webhooks", "github"])
            && Self::path_segment_count(path) == 4
        {
            return Some(RouteTarget {
                class: RateLimitClass::Public,
                bucket: "webhooks.github.named",
            });
        }

        if Self::path_matches(path, &["v1", "github", "install"]) {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedRead,
                bucket: "github.install",
            });
        }

        if Self::path_matches(path, &["v1", "github", "repos"]) {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedRead,
                bucket: "github.repos",
            });
        }

        if Self::path_matches(path, &["v1", "github", "accounts"]) {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedRead,
                bucket: "github.accounts",
            });
        }

        if Self::path_matches(path, &["v1", "auth", "me"]) && method == Method::GET {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedRead,
                bucket: "auth.me.get",
            });
        }

        if Self::path_matches(path, &["v1", "auth", "me"]) && method == Method::PUT {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedWrite,
                bucket: "auth.me.put",
            });
        }

        if Self::path_matches(path, &["v1", "apps"]) && method == Method::GET {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedRead,
                bucket: "apps.list",
            });
        }

        if Self::path_matches(path, &["v1", "apps"]) && method == Method::POST {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedWrite,
                bucket: "apps.create",
            });
        }

        if Self::path_matches(path, &["v1", "deploy"]) && method == Method::POST {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedWrite,
                bucket: "deploy.global",
            });
        }

        if Self::path_has_prefix(path, &["v1", "apps"]) && Self::path_segment_count(path) == 4 {
            if method == Method::DELETE {
                return Some(RouteTarget {
                    class: RateLimitClass::AuthenticatedWrite,
                    bucket: "apps.delete",
                });
            }

            if method == Method::GET && path.ends_with("/secret") {
                return Some(RouteTarget {
                    class: RateLimitClass::AuthenticatedRead,
                    bucket: "apps.secret.get",
                });
            }

            if method == Method::POST && path.ends_with("/deploy") {
                return Some(RouteTarget {
                    class: RateLimitClass::AuthenticatedWrite,
                    bucket: "apps.deploy.trigger",
                });
            }
        }

        if Self::path_has_prefix(path, &["v1", "apps"])
            && Self::path_segment_count(path) == 4
            && path.ends_with("/deployments")
        {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedRead,
                bucket: "apps.deployments.list",
            });
        }

        if Self::path_has_prefix(path, &["v1", "apps"])
            && Self::path_segment_count(path) == 5
            && path.ends_with("/deployments/stream")
        {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedStream,
                bucket: "apps.deployments.stream",
            });
        }

        if Self::path_has_prefix(path, &["v1", "apps"])
            && Self::path_segment_count(path) == 5
            && path.ends_with("/logs/stream")
        {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedStream,
                bucket: "apps.logs.stream",
            });
        }

        if Self::path_has_prefix(path, &["v1", "apps"])
            && Self::path_segment_count(path) == 5
            && path.ends_with("/metrics/stream")
        {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedStream,
                bucket: "apps.metrics.stream",
            });
        }

        if Self::path_has_prefix(path, &["v1", "apps"])
            && Self::path_segment_count(path) == 6
            && path.ends_with("/activate")
            && method == Method::POST
        {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedWrite,
                bucket: "apps.deployments.activate",
            });
        }

        if Self::path_has_prefix(path, &["v1", "apps"]) && Self::path_segment_count(path) == 5 {
            if method == Method::GET {
                return Some(RouteTarget {
                    class: RateLimitClass::AuthenticatedRead,
                    bucket: "apps.deployments.status",
                });
            }

            if method == Method::DELETE {
                return Some(RouteTarget {
                    class: RateLimitClass::AuthenticatedWrite,
                    bucket: "apps.deployments.stop",
                });
            }
        }

        if Self::path_has_prefix(path, &["v1", "apps"])
            && Self::path_segment_count(path) == 6
            && path.ends_with("/logs")
            && method == Method::GET
        {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedStream,
                bucket: "apps.deployments.logs",
            });
        }

        if Self::path_has_prefix(path, &["v1", "apps"])
            && Self::path_segment_count(path) == 6
            && path.ends_with("/pause")
            && method == Method::POST
        {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedWrite,
                bucket: "apps.deployments.pause",
            });
        }

        if Self::path_has_prefix(path, &["v1", "apps"])
            && Self::path_segment_count(path) == 6
            && path.ends_with("/resume")
            && method == Method::POST
        {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedWrite,
                bucket: "apps.deployments.resume",
            });
        }

        if Self::path_has_prefix(path, &["v1", "apps"])
            && Self::path_segment_count(path) == 6
            && path.ends_with("/delete")
            && method == Method::DELETE
        {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedWrite,
                bucket: "apps.deployments.delete",
            });
        }

        if Self::path_has_prefix(path, &["v1", "apps"])
            && Self::path_segment_count(path) == 4
            && path.ends_with("/security-groups")
        {
            if method == Method::GET {
                return Some(RouteTarget {
                    class: RateLimitClass::AuthenticatedRead,
                    bucket: "apps.security_groups.list",
                });
            }

            if method == Method::POST {
                return Some(RouteTarget {
                    class: RateLimitClass::AuthenticatedWrite,
                    bucket: "apps.security_groups.create",
                });
            }
        }

        if Self::path_has_prefix(path, &["v1", "apps"])
            && Self::path_segment_count(path) == 5
            && path.contains("/security-groups/")
            && method == Method::DELETE
        {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedWrite,
                bucket: "apps.security_groups.delete",
            });
        }

        if Self::path_matches(path, &["v1", "networking", "mesh"]) && method == Method::GET {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedRead,
                bucket: "networking.mesh",
            });
        }

        if Self::path_matches(path, &["v1", "networking", "mesh", "stream"])
            && method == Method::GET
        {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedStream,
                bucket: "networking.mesh.stream",
            });
        }

        if Self::path_matches(path, &["v1", "deployments", "active"]) && method == Method::GET {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedRead,
                bucket: "deployments.active",
            });
        }

        if Self::path_matches(path, &["v1", "deployments", "events"]) && method == Method::GET {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedStream,
                bucket: "deployments.events",
            });
        }

        if Self::path_matches(path, &["v1", "workspace", "events"]) && method == Method::GET {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedStream,
                bucket: "workspace.events",
            });
        }

        if method == Method::GET || method == Method::HEAD {
            return Some(RouteTarget {
                class: RateLimitClass::AuthenticatedRead,
                bucket: "unclassified.read",
            });
        }

        Some(RouteTarget {
            class: RateLimitClass::AuthenticatedWrite,
            bucket: "unclassified.write",
        })
    }

    fn derive_key(&self, request: &Request<Body>, target: &RouteTarget) -> RateLimitKey {
        match target.class {
            RateLimitClass::Public => RateLimitKey {
                bucket: target.bucket,
                identity: self.client_identity(request),
            },
            RateLimitClass::AuthenticatedRead
            | RateLimitClass::AuthenticatedWrite
            | RateLimitClass::AuthenticatedStream => {
                if let Some(claims) = request.extensions().get::<crate::auth::jwt::Claims>() {
                    return RateLimitKey {
                        bucket: target.bucket,
                        identity: RateLimitIdentity::UserId(claims.sub.clone()),
                    };
                }

                if let Ok(token) =
                    extract_token_from_headers_and_uri(request.headers(), request.uri())
                    && let Ok(claims) = jwt::verify_token(&token, &self.jwt_secret)
                {
                    return RateLimitKey {
                        bucket: target.bucket,
                        identity: RateLimitIdentity::UserId(claims.sub),
                    };
                }

                RateLimitKey {
                    bucket: target.bucket,
                    identity: self.client_identity(request),
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

    fn client_identity(&self, request: &Request<Body>) -> RateLimitIdentity {
        if self.config.trust_proxy_headers
            && let Some(ip) = forwarded_for_ip(request.headers())
        {
            return RateLimitIdentity::ForwardedIp(ip);
        }

        request
            .extensions()
            .get::<axum::extract::ConnectInfo<SocketAddr>>()
            .map(|ci| RateLimitIdentity::SocketAddr(ci.0))
            .unwrap_or(RateLimitIdentity::Unknown)
    }

    fn get_bucket(
        &self,
        key: RateLimitKey,
        policy: &BucketPolicy,
    ) -> Arc<std::sync::Mutex<BucketState>> {
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

    fn check_bucket(&self, key: RateLimitKey, policy: &BucketPolicy) -> RateLimitOutcome {
        let bucket = self.get_bucket(key, policy);
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
            let reset_at = SystemTime::now()
                + if policy.refill_per_second <= f64::EPSILON {
                    Duration::from_secs(0)
                } else {
                    Duration::from_secs_f64(
                        ((policy.capacity - guard.tokens).max(0.0)) / policy.refill_per_second,
                    )
                };
            return RateLimitOutcome {
                allowed: true,
                retry_after: Duration::from_secs(0),
                limit: policy.capacity as u32,
                remaining,
                reset_at,
            };
        }

        let missing = 1.0 - guard.tokens;
        let retry_secs = if policy.refill_per_second <= f64::EPSILON {
            1.0
        } else {
            (missing / policy.refill_per_second).ceil().max(1.0)
        };

        let reset_secs = if policy.refill_per_second <= f64::EPSILON {
            1.0
        } else {
            (policy.capacity - guard.tokens).max(0.0) / policy.refill_per_second
        };

        RateLimitOutcome {
            allowed: false,
            retry_after: Duration::from_secs(retry_secs as u64),
            limit: policy.capacity as u32,
            remaining: 0,
            reset_at: SystemTime::now() + Duration::from_secs_f64(reset_secs),
        }
    }

    fn evaluate(&self, request: &Request<Body>) -> Option<RateLimitOutcome> {
        let target = Self::classify_request(request.method(), request.uri().path())?;
        let policy = self.policy_for_target(&target);
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
    mut request: Request<Body>,
    next: Next,
) -> Response {
    if let Some(claims) = request
        .extensions()
        .get::<crate::auth::jwt::Claims>()
        .cloned()
    {
        request.extensions_mut().insert(claims);
    } else if let Ok(token) = extract_token_from_headers_and_uri(request.headers(), request.uri())
        && let Ok(claims) = jwt::verify_token(&token, &rate_limiter.jwt_secret)
    {
        request.extensions_mut().insert(claims);
    }

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
            outcome.reset_at,
        );
        return response;
    }

    let mut response = next.run(request).await;
    if let Some(outcome) = outcome {
        insert_rate_limit_headers(
            response.headers_mut(),
            outcome.limit,
            outcome.remaining,
            outcome.reset_at,
        );
    }
    response
}

fn insert_rate_limit_headers(
    headers: &mut axum::http::HeaderMap,
    limit: u32,
    remaining: u32,
    reset_at: SystemTime,
) {
    let limit_value = HeaderValue::from_str(&limit.to_string());
    let remaining_value = HeaderValue::from_str(&remaining.to_string());
    let reset_value = reset_at
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| HeaderValue::from_str(&duration.as_secs().to_string()).ok());

    if let Ok(value) = limit_value {
        headers.insert("x-ratelimit-limit", value);
    }
    if let Ok(value) = remaining_value {
        headers.insert("x-ratelimit-remaining", value);
    }
    if let Some(value) = reset_value {
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
    fn unclassified_paths_use_fixed_buckets() {
        let first = RateLimiter::classify_request(&Method::GET, "/v1/random-a").unwrap();
        let second = RateLimiter::classify_request(&Method::GET, "/v1/random-b").unwrap();
        let third = RateLimiter::classify_request(&Method::POST, "/v1/random-c").unwrap();

        assert_eq!(first.class, RateLimitClass::AuthenticatedRead);
        assert_eq!(second.class, RateLimitClass::AuthenticatedRead);
        assert_eq!(third.class, RateLimitClass::AuthenticatedWrite);
        assert_eq!(first.bucket, "unclassified.read");
        assert_eq!(second.bucket, "unclassified.read");
        assert_eq!(third.bucket, "unclassified.write");
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
                cleanup_interval: Duration::from_secs(1),
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
                cleanup_interval: Duration::from_secs(1),
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
                cleanup_interval: Duration::from_secs(1),
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
                cleanup_interval: Duration::from_secs(1),
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

    #[test]
    fn rate_limit_reset_header_uses_unix_timestamp() {
        let mut headers = axum::http::HeaderMap::new();
        insert_rate_limit_headers(
            &mut headers,
            10,
            9,
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        );

        assert_eq!(headers["x-ratelimit-reset"], "1700000000");
    }
}
