use crate::state::State;
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct StateManager {
    state: Arc<RwLock<State>>,
}

impl StateManager {
    pub fn new(_cache_path: PathBuf) -> Result<Self> {
        Ok(Self {
            state: Arc::new(RwLock::new(State::default())),
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

            if let Ok(x509) = X509::from_pem(cert.cert_pem.as_bytes()) {
                cert.parsed_cert = Some(x509);
            }
            if let Ok(pkey) = PKey::private_key_from_pem(cert.key_pem.as_bytes()) {
                cert.parsed_key = Some(pkey);
            }
        }

        let mut state = self.state.write().await;
        *state = new_state;
        drop(state);
        Ok(())
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
