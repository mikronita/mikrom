use crate::state::{Certificate, Route, State};
use anyhow::Result;
use pingora::lb::LoadBalancer;
use pingora::lb::selection::RoundRobin;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

#[derive(Serialize, Deserialize)]
struct SerializableState {
    pub routes: HashMap<String, Vec<String>>, // host -> targets
    pub acme_tokens: HashMap<String, String>,
    pub certificates: HashMap<String, Certificate>,
}

pub struct StateManager {
    state: Arc<RwLock<State>>,
    cache_path: PathBuf,
}

impl StateManager {
    pub fn new(cache_path: PathBuf) -> Result<Self> {
        let mut initial_state = State::default();

        if cache_path.exists() {
            info!("Loading state from cache: {:?}", cache_path);
            match std::fs::read_to_string(&cache_path) {
                Ok(content) => match serde_json::from_str::<SerializableState>(&content) {
                    Ok(s_state) => {
                        for (host, targets) in s_state.routes {
                            if let Ok(lb) =
                                LoadBalancer::<RoundRobin>::try_from_iter(targets.as_slice())
                            {
                                initial_state.routes.insert(
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
                        initial_state.acme_tokens = s_state.acme_tokens;
                        initial_state.certificates = s_state.certificates;

                        // Pre-parse certificates
                        for cert in initial_state.certificates.values_mut() {
                            use openssl::pkey::PKey;
                            use openssl::x509::X509;

                            if let Ok(chain) = X509::stack_from_pem(cert.cert_pem.as_bytes()) {
                                cert.parsed_chain = chain;
                            }
                            if let Ok(pkey) = PKey::private_key_from_pem(cert.key_pem.as_bytes()) {
                                cert.parsed_key = Some(pkey);
                            }
                        }
                        info!("Successfully restored state from cache.");
                    },
                    Err(e) => warn!("Failed to parse state cache: {e}"),
                },
                Err(e) => warn!("Failed to read state cache: {e}"),
            }
        }

        Ok(Self {
            state: Arc::new(RwLock::new(initial_state)),
            cache_path,
        })
    }

    #[must_use]
    pub fn get_state(&self) -> Arc<RwLock<State>> {
        self.state.clone()
    }

    pub async fn update_state(&self, mut new_state: State) -> Result<()> {
        // Pre-parse certificates for performance
        for cert in new_state.certificates.values_mut() {
            use openssl::pkey::PKey;
            use openssl::x509::X509;

            if let Ok(chain) = X509::stack_from_pem(cert.cert_pem.as_bytes()) {
                cert.parsed_chain = chain;
            }
            if let Ok(pkey) = PKey::private_key_from_pem(cert.key_pem.as_bytes()) {
                cert.parsed_key = Some(pkey);
            }
        }

        // Persist to disk
        self.persist_state(&new_state).await;

        let mut state = self.state.write().await;
        *state = new_state;
        drop(state);
        Ok(())
    }

    pub async fn add_acme_token(&self, token: String, key_auth: String) -> Result<()> {
        let mut state = self.state.write().await;
        state.acme_tokens.insert(token, key_auth);
        let state_clone = state.clone();
        drop(state);

        self.persist_state(&state_clone).await;
        Ok(())
    }

    pub async fn remove_acme_token(&self, token: &str) -> Result<()> {
        let mut state = self.state.write().await;
        state.acme_tokens.remove(token);
        let state_clone = state.clone();
        drop(state);

        self.persist_state(&state_clone).await;
        Ok(())
    }

    pub async fn add_certificate(
        &self,
        domain: String,
        cert_pem: String,
        key_pem: String,
    ) -> Result<()> {
        use openssl::pkey::PKey;
        use openssl::x509::X509;

        let mut cert = Certificate {
            cert_pem,
            key_pem,
            parsed_chain: Vec::new(),
            parsed_key: None,
        };

        // Pre-parse certificate
        if let Ok(chain) = X509::stack_from_pem(cert.cert_pem.as_bytes()) {
            cert.parsed_chain = chain;
        }
        if let Ok(pkey) = PKey::private_key_from_pem(cert.key_pem.as_bytes()) {
            cert.parsed_key = Some(pkey);
        }

        let mut state = self.state.write().await;
        state.certificates.insert(domain, cert);
        let state_clone = state.clone();
        drop(state);

        self.persist_state(&state_clone).await;
        Ok(())
    }

    async fn persist_state(&self, state: &State) {
        let s_state = SerializableState {
            routes: state
                .routes
                .iter()
                .map(|(k, v)| (k.clone(), v.targets.clone()))
                .collect(),
            acme_tokens: state.acme_tokens.clone(),
            certificates: state.certificates.clone(),
        };

        match serde_json::to_string_pretty(&s_state) {
            Ok(json) => {
                if let Err(e) = tokio::fs::write(&self.cache_path, json).await {
                    error!("Failed to write state cache to {:?}: {e}", self.cache_path);
                }
            },
            Err(e) => error!("Failed to serialize state for cache: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Route;
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
}
