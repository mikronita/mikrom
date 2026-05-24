use crate::domain::state::{Certificate, Route, State};
use anyhow::Result;
use pingora::lb::LoadBalancer;
use pingora::lb::selection::RoundRobin;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

#[derive(Serialize, Deserialize, Default)]
struct SerializableState {
    pub routes: HashMap<String, Vec<String>>, // host -> targets
    pub acme_tokens: HashMap<String, String>,
    pub certificates: HashMap<String, Certificate>,
    #[serde(default)]
    pub route_versions: HashMap<String, i64>,
    #[serde(default)]
    pub acme_versions: HashMap<String, i64>,
    #[serde(default)]
    pub certificate_versions: HashMap<String, i64>,
}

impl From<&State> for SerializableState {
    fn from(state: &State) -> Self {
        Self::from_snapshot(state, &StateVersions::default())
    }
}

impl SerializableState {
    fn from_snapshot(state: &State, versions: &StateVersions) -> Self {
        Self {
            routes: state
                .routes
                .iter()
                .map(|(host, route)| (host.clone(), route.targets.clone()))
                .collect(),
            acme_tokens: state.acme_tokens.clone(),
            certificates: state.certificates.clone(),
            route_versions: versions.route_versions.clone(),
            acme_versions: versions.acme_versions.clone(),
            certificate_versions: versions.certificate_versions.clone(),
        }
    }

    fn into_snapshot(self) -> (State, StateVersions) {
        let mut state = State::default();

        for (host, targets) in self.routes {
            if let Ok(lb) = LoadBalancer::<RoundRobin>::try_from_iter(targets.as_slice()) {
                state.routes.insert(
                    host.clone(),
                    Route {
                        host,
                        targets,
                        lb: Arc::new(lb),
                        use_tls: false,
                        tls_alternative_cn: None,
                    },
                );
            }
        }

        state.acme_tokens = self.acme_tokens;
        state.certificates = self.certificates;
        StateManager::prepare_certificates(&mut state.certificates);

        (
            state,
            StateVersions {
                route_versions: self.route_versions,
                acme_versions: self.acme_versions,
                certificate_versions: self.certificate_versions,
            },
        )
    }
}

#[derive(Clone, Default, Debug, PartialEq, Eq)]
#[allow(clippy::struct_field_names)]
pub(crate) struct StateVersions {
    pub(crate) route_versions: HashMap<String, i64>,
    pub(crate) acme_versions: HashMap<String, i64>,
    pub(crate) certificate_versions: HashMap<String, i64>,
}

impl StateVersions {
    #[must_use]
    pub(crate) fn route_version(&self, host: &str) -> Option<i64> {
        self.route_versions.get(host).copied()
    }

    #[must_use]
    pub(crate) fn acme_version(&self, token: &str) -> Option<i64> {
        self.acme_versions.get(token).copied()
    }

    #[must_use]
    pub(crate) fn certificate_version(&self, domain: &str) -> Option<i64> {
        self.certificate_versions.get(domain).copied()
    }
}

#[derive(Clone, Default)]
struct StateSnapshot {
    state: State,
    versions: StateVersions,
}

pub struct StateManager {
    state: Arc<RwLock<State>>,
    versions: Arc<RwLock<StateVersions>>,
    snapshot: Arc<RwLock<StateSnapshot>>,
    cache_path: PathBuf,
}

impl StateManager {
    pub fn new(cache_path: PathBuf) -> Result<Self> {
        let (initial_state, initial_versions) = Self::load_cached_state(&cache_path);
        let snapshot = StateSnapshot {
            state: initial_state.clone(),
            versions: initial_versions.clone(),
        };

        Ok(Self {
            state: Arc::new(RwLock::new(initial_state)),
            versions: Arc::new(RwLock::new(initial_versions)),
            snapshot: Arc::new(RwLock::new(snapshot)),
            cache_path,
        })
    }

    #[must_use]
    pub fn get_state(&self) -> Arc<RwLock<State>> {
        self.state.clone()
    }

    pub(crate) async fn snapshot(&self) -> (State, StateVersions) {
        let snapshot = self.snapshot.read().await;
        (snapshot.state.clone(), snapshot.versions.clone())
    }

    pub async fn update_state(&self, new_state: State) -> Result<()> {
        self.replace_state(new_state, StateVersions::default())
            .await
    }

    pub(crate) async fn replace_state(
        &self,
        mut new_state: State,
        versions: StateVersions,
    ) -> Result<()> {
        Self::prepare_certificates(&mut new_state.certificates);

        let state_clone = new_state.clone();
        let versions_clone = versions.clone();

        {
            let mut state = self.state.write().await;
            *state = new_state;
        }
        {
            let mut current_versions = self.versions.write().await;
            *current_versions = versions;
        }

        self.update_snapshot_cache(&state_clone, &versions_clone)
            .await;
        self.persist_snapshot(&state_clone, &versions_clone).await;
        Ok(())
    }

    pub async fn add_acme_token(
        &self,
        token: String,
        key_auth: String,
        timestamp: i64,
    ) -> Result<bool> {
        if !self
            .should_apply_if_newer(|versions| versions.acme_version(&token), timestamp)
            .await
        {
            return Ok(false);
        }

        let (state_clone, versions_clone) = {
            let mut state = self.state.write().await;
            state.acme_tokens.insert(token.clone(), key_auth);
            let state_clone = state.clone();
            drop(state);

            let mut versions = self.versions.write().await;
            versions.acme_versions.insert(token, timestamp);
            let versions_clone = versions.clone();
            drop(versions);

            (state_clone, versions_clone)
        };

        self.update_snapshot_cache(&state_clone, &versions_clone)
            .await;
        self.persist_snapshot(&state_clone, &versions_clone).await;
        Ok(true)
    }

    pub async fn remove_acme_token(&self, token: &str, timestamp: i64) -> Result<bool> {
        if !self
            .should_apply_if_newer(|versions| versions.acme_version(token), timestamp)
            .await
        {
            return Ok(false);
        }

        let token = token.to_string();
        let (state_clone, versions_clone) = {
            let mut state = self.state.write().await;
            state.acme_tokens.remove(&token);
            let state_clone = state.clone();
            drop(state);

            let mut versions = self.versions.write().await;
            versions.acme_versions.insert(token, timestamp);
            let versions_clone = versions.clone();
            drop(versions);

            (state_clone, versions_clone)
        };

        self.update_snapshot_cache(&state_clone, &versions_clone)
            .await;
        self.persist_snapshot(&state_clone, &versions_clone).await;
        Ok(true)
    }

    pub async fn add_certificate(
        &self,
        domain: String,
        cert_pem: String,
        key_pem: String,
        timestamp: i64,
    ) -> Result<bool> {
        if !self
            .should_apply_if_newer(|versions| versions.certificate_version(&domain), timestamp)
            .await
        {
            return Ok(false);
        }

        let mut cert = Certificate {
            cert_pem,
            key_pem,
            parsed_chain: Vec::new(),
            parsed_key: None,
        };

        Self::prepare_certificate(&mut cert);

        let (state_clone, versions_clone) = {
            let mut state = self.state.write().await;
            state.certificates.insert(domain.clone(), cert);
            let state_clone = state.clone();
            drop(state);

            let mut versions = self.versions.write().await;
            versions.certificate_versions.insert(domain, timestamp);
            let versions_clone = versions.clone();
            drop(versions);

            (state_clone, versions_clone)
        };

        self.update_snapshot_cache(&state_clone, &versions_clone)
            .await;
        self.persist_snapshot(&state_clone, &versions_clone).await;
        Ok(true)
    }

    pub async fn update_route_targets(
        &self,
        host: String,
        targets: Vec<String>,
        timestamp: i64,
    ) -> Result<bool> {
        use pingora::lb::health_check::TcpHealthCheck;

        if !self
            .should_apply_if_newer(|versions| versions.route_version(&host), timestamp)
            .await
        {
            return Ok(false);
        }

        if targets.is_empty() {
            let host_key = host.clone();
            let (state_clone, versions_clone) = {
                let mut state = self.state.write().await;
                state.routes.remove(&host_key);
                let state_clone = state.clone();
                drop(state);

                let mut versions = self.versions.write().await;
                versions.route_versions.insert(host_key, timestamp);
                let versions_clone = versions.clone();
                drop(versions);

                (state_clone, versions_clone)
            };

            self.update_snapshot_cache(&state_clone, &versions_clone)
                .await;
            self.persist_snapshot(&state_clone, &versions_clone).await;
            return Ok(true);
        }

        let mut lb = LoadBalancer::<RoundRobin>::try_from_iter(targets.as_slice())?;

        let mut hc = TcpHealthCheck::default();
        hc.consecutive_success = 1;
        hc.consecutive_failure = 2;

        lb.set_health_check(Box::new(hc));
        lb.health_check_frequency = Some(std::time::Duration::from_millis(250));

        let host_key = host.clone();
        let (state_clone, versions_clone) = {
            let mut state = self.state.write().await;
            let (use_tls, alternative_cn) = state
                .routes
                .get(&host_key)
                .map_or((false, None), |r| (r.use_tls, r.tls_alternative_cn.clone()));

            state.routes.insert(
                host_key.clone(),
                Route {
                    host,
                    targets,
                    lb: Arc::new(lb),
                    use_tls,
                    tls_alternative_cn: alternative_cn,
                },
            );

            let state_clone = state.clone();
            drop(state);

            let mut versions = self.versions.write().await;
            versions.route_versions.insert(host_key, timestamp);
            let versions_clone = versions.clone();
            drop(versions);

            (state_clone, versions_clone)
        };

        self.update_snapshot_cache(&state_clone, &versions_clone)
            .await;
        self.persist_snapshot(&state_clone, &versions_clone).await;
        Ok(true)
    }

    async fn persist_snapshot(&self, state: &State, versions: &StateVersions) {
        let s_state = SerializableState::from_snapshot(state, versions);

        match serde_json::to_string_pretty(&s_state) {
            Ok(json) => {
                let suffix = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0_u128, |duration| duration.as_nanos());
                let temp_path = self.cache_path.with_extension(format!("tmp.{suffix:x}"));

                if let Err(e) = tokio::fs::write(&temp_path, json).await {
                    error!("Failed to write state cache to {:?}: {e}", temp_path);
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    return;
                }

                if let Err(e) = tokio::fs::rename(&temp_path, &self.cache_path).await {
                    error!(
                        "Failed to move state cache from {:?} to {:?}: {e}",
                        temp_path, self.cache_path
                    );
                    let _ = tokio::fs::remove_file(&temp_path).await;
                }
            },
            Err(e) => error!("Failed to serialize state for cache: {e}"),
        }
    }

    fn load_cached_state(cache_path: &PathBuf) -> (State, StateVersions) {
        let initial_state = State::default();
        let initial_versions = StateVersions::default();

        if !cache_path.exists() {
            return (initial_state, initial_versions);
        }

        info!("Loading state from cache: {:?}", cache_path);
        let content = match std::fs::read_to_string(cache_path) {
            Ok(content) => content,
            Err(e) => {
                warn!("Failed to read state cache: {e}");
                return (initial_state, initial_versions);
            },
        };

        let s_state = match serde_json::from_str::<SerializableState>(&content) {
            Ok(s_state) => s_state,
            Err(e) => {
                warn!("Failed to parse state cache: {e}");
                return (initial_state, initial_versions);
            },
        };

        let (state, versions) = s_state.into_snapshot();

        info!(
            routes = state.routes.len(),
            acme_tokens = state.acme_tokens.len(),
            certificates = state.certificates.len(),
            "Successfully restored state from cache"
        );
        (state, versions)
    }

    async fn should_apply_if_newer<F>(&self, current_version: F, timestamp: i64) -> bool
    where
        F: FnOnce(&StateVersions) -> Option<i64>,
    {
        let versions = self.versions.read().await;
        !matches!(current_version(&versions), Some(current) if timestamp <= current)
    }

    async fn update_snapshot_cache(&self, state: &State, versions: &StateVersions) {
        let mut snapshot = self.snapshot.write().await;
        snapshot.state = state.clone();
        snapshot.versions = versions.clone();
    }

    fn prepare_certificates(certificates: &mut HashMap<String, Certificate>) {
        for cert in certificates.values_mut() {
            Self::prepare_certificate(cert);
        }
    }

    fn prepare_certificate(cert: &mut Certificate) {
        use openssl::pkey::PKey;
        use openssl::x509::X509;

        cert.parsed_chain = X509::stack_from_pem(cert.cert_pem.as_bytes()).unwrap_or_default();
        cert.parsed_key = PKey::private_key_from_pem(cert.key_pem.as_bytes()).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::state::Route;
    use pingora::lb::LoadBalancer;
    use pingora::lb::selection::RoundRobin;
    use std::collections::HashMap;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_state_manager_logic() {
        let temp_file = NamedTempFile::new().unwrap();
        let cache_path = temp_file.path().to_path_buf();

        let mut routes = HashMap::new();
        let targets = vec!["[fd00::1]:8080".to_string()];
        let lb = LoadBalancer::<RoundRobin>::try_from_iter(targets.as_slice()).unwrap();

        routes.insert(
            "test.mikrom.local".to_string(),
            Route {
                host: "test.mikrom.local".to_string(),
                targets: targets.clone(),
                lb: Arc::new(lb),
                use_tls: false,
                tls_alternative_cn: None,
            },
        );

        let initial_state = State {
            routes,
            acme_tokens: HashMap::new(),
            certificates: HashMap::new(),
        };

        let manager = StateManager::new(cache_path).unwrap();
        manager.update_state(initial_state.clone()).await.unwrap();

        let state_arc = manager.get_state();
        let state = state_arc.read().await;
        assert_eq!(state.routes.len(), 1);
        assert_eq!(
            state.routes.get("test.mikrom.local").unwrap().targets[0],
            "[fd00::1]:8080"
        );
        drop(state);
    }

    #[tokio::test]
    async fn test_state_manager_empty_initial() {
        let temp_file = NamedTempFile::new().unwrap();
        let cache_path = temp_file.path().to_path_buf();

        let manager = StateManager::new(cache_path).unwrap();
        let state_arc = manager.get_state();
        let state = state_arc.read().await;
        assert!(state.routes.is_empty());
        assert!(state.acme_tokens.is_empty());
        drop(state);
    }

    #[test]
    fn serializable_state_round_trips_routes_and_certificates() {
        let state = State {
            routes: {
                let mut routes = HashMap::new();
                let targets = vec!["127.0.0.1:8080".to_string()];
                let lb = LoadBalancer::<RoundRobin>::try_from_iter(targets.as_slice()).unwrap();
                routes.insert(
                    "roundtrip.example.com".to_string(),
                    Route {
                        host: "roundtrip.example.com".to_string(),
                        targets,
                        lb: Arc::new(lb),
                        use_tls: true,
                        tls_alternative_cn: Some("alt.example.com".to_string()),
                    },
                );
                routes
            },
            acme_tokens: {
                let mut tokens = HashMap::new();
                tokens.insert("token".to_string(), "key-auth".to_string());
                tokens
            },
            certificates: {
                let mut certs = HashMap::new();
                certs.insert(
                    "roundtrip.example.com".to_string(),
                    Certificate {
                        cert_pem: "invalid-cert".to_string(),
                        key_pem: "invalid-key".to_string(),
                        parsed_chain: Vec::new(),
                        parsed_key: None,
                    },
                );
                certs
            },
        };

        let serialized = serde_json::to_string(&SerializableState::from(&state)).unwrap();
        let loaded = serde_json::from_str::<SerializableState>(&serialized).unwrap();

        assert_eq!(
            loaded.routes["roundtrip.example.com"],
            vec!["127.0.0.1:8080"]
        );
        assert_eq!(loaded.acme_tokens["token"], "key-auth");
        assert_eq!(
            loaded.certificates["roundtrip.example.com"].cert_pem,
            "invalid-cert"
        );
    }

    #[test]
    fn prepare_certificate_keeps_invalid_pem_non_fatal() {
        let mut cert = Certificate {
            cert_pem: "not-a-cert".to_string(),
            key_pem: "not-a-key".to_string(),
            parsed_chain: Vec::new(),
            parsed_key: None,
        };

        StateManager::prepare_certificate(&mut cert);

        assert!(cert.parsed_chain.is_empty());
        assert!(cert.parsed_key.is_none());
    }

    #[tokio::test]
    async fn stale_route_update_is_ignored() {
        let temp_file = NamedTempFile::new().unwrap();
        let cache_path = temp_file.path().to_path_buf();
        let manager = StateManager::new(cache_path).unwrap();

        let applied = manager
            .update_route_targets(
                "router.example.com".to_string(),
                vec!["127.0.0.1:8080".to_string()],
                10,
            )
            .await
            .unwrap();
        assert!(applied);

        let ignored = manager
            .update_route_targets(
                "router.example.com".to_string(),
                vec!["127.0.0.1:9090".to_string()],
                5,
            )
            .await
            .unwrap();
        assert!(!ignored);

        {
            let state = manager.get_state();
            let state = state.read().await;
            assert_eq!(
                state.routes["router.example.com"].targets,
                vec!["127.0.0.1:8080"]
            );
            drop(state);
        }
    }

    #[tokio::test]
    async fn stale_certificate_update_is_ignored() {
        let temp_file = NamedTempFile::new().unwrap();
        let cache_path = temp_file.path().to_path_buf();
        let manager = StateManager::new(cache_path).unwrap();

        let applied = manager
            .add_certificate(
                "cert.example.com".to_string(),
                "cert-v1".to_string(),
                "key-v1".to_string(),
                10,
            )
            .await
            .unwrap();
        assert!(applied);

        let ignored = manager
            .add_certificate(
                "cert.example.com".to_string(),
                "cert-v0".to_string(),
                "key-v0".to_string(),
                5,
            )
            .await
            .unwrap();
        assert!(!ignored);

        let cert_pem = {
            let state = manager.get_state();
            let state = state.read().await;
            let cert_pem = state.certificates["cert.example.com"].cert_pem.clone();
            drop(state);
            cert_pem
        };
        assert_eq!(cert_pem, "cert-v1");
    }

    #[tokio::test]
    async fn stale_acme_delete_is_ignored() {
        let temp_file = NamedTempFile::new().unwrap();
        let cache_path = temp_file.path().to_path_buf();
        let manager = StateManager::new(cache_path).unwrap();

        let applied = manager
            .add_acme_token("token".to_string(), "key-auth".to_string(), 10)
            .await
            .unwrap();
        assert!(applied);

        let ignored = manager.remove_acme_token("token", 5).await.unwrap();
        assert!(!ignored);

        let key_auth = {
            let state = manager.get_state();
            let state = state.read().await;
            let key_auth = state.acme_tokens["token"].clone();
            drop(state);
            key_auth
        };
        assert_eq!(key_auth, "key-auth");
    }

    #[tokio::test]
    async fn versions_snapshot_tracks_latest_entity_revisions() {
        let temp_file = NamedTempFile::new().unwrap();
        let cache_path = temp_file.path().to_path_buf();
        let manager = StateManager::new(cache_path).unwrap();

        assert!(
            manager
                .update_route_targets(
                    "route.example.com".to_string(),
                    vec!["127.0.0.1:8080".to_string()],
                    11,
                )
                .await
                .unwrap()
        );
        assert!(
            manager
                .add_acme_token("token".to_string(), "key-auth".to_string(), 12)
                .await
                .unwrap()
        );
        assert!(
            manager
                .add_certificate(
                    "cert.example.com".to_string(),
                    "cert-v1".to_string(),
                    "key-v1".to_string(),
                    13,
                )
                .await
                .unwrap()
        );

        let (_, versions) = manager.snapshot().await;
        assert_eq!(versions.route_version("route.example.com"), Some(11));
        assert_eq!(versions.acme_version("token"), Some(12));
        assert_eq!(versions.certificate_version("cert.example.com"), Some(13));
    }

    #[tokio::test]
    async fn corrupt_cache_falls_back_to_empty_state() {
        let temp_file = NamedTempFile::new().unwrap();
        std::fs::write(temp_file.path(), "{not valid json").unwrap();

        let manager = StateManager::new(temp_file.path().to_path_buf()).unwrap();
        {
            let state_arc = manager.get_state();
            let state = state_arc.read().await;

            assert!(state.routes.is_empty());
            assert!(state.acme_tokens.is_empty());
            assert!(state.certificates.is_empty());
            drop(state);
        }
    }

    #[tokio::test]
    async fn persisted_cache_is_restored_after_restart() {
        let temp_file = NamedTempFile::new().unwrap();
        let cache_path = temp_file.path().to_path_buf();

        let manager = StateManager::new(cache_path.clone()).unwrap();
        assert!(
            manager
                .update_route_targets(
                    "restart.example.com".to_string(),
                    vec!["127.0.0.1:8080".to_string()],
                    21,
                )
                .await
                .unwrap()
        );
        assert!(
            manager
                .add_acme_token("token-restart".to_string(), "key-auth".to_string(), 22)
                .await
                .unwrap()
        );
        assert!(
            manager
                .add_certificate(
                    "restart.example.com".to_string(),
                    "cert-v1".to_string(),
                    "key-v1".to_string(),
                    23,
                )
                .await
                .unwrap()
        );

        drop(manager);

        let manager = StateManager::new(cache_path).unwrap();
        {
            let state_arc = manager.get_state();
            let state = state_arc.read().await;

            assert!(state.routes.contains_key("restart.example.com"));
            assert_eq!(
                state.acme_tokens.get("token-restart").map(String::as_str),
                Some("key-auth")
            );
            assert_eq!(
                state
                    .certificates
                    .get("restart.example.com")
                    .map(|cert| cert.cert_pem.as_str()),
                Some("cert-v1")
            );
            drop(state);
        }
    }
}
