use crate::state::State;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::RwLock;
use tracing::{info, warn};

pub struct StateManager {
    cache_path: PathBuf,
    state: Arc<RwLock<State>>,
}

impl StateManager {
    pub async fn new(cache_path: PathBuf) -> Result<Self> {
        let state = if cache_path.exists() {
            info!("Loading state from cache: {:?}", cache_path);
            let data = fs::read(&cache_path).await?;
            serde_json::from_slice(&data).unwrap_or_else(|e| {
                warn!(
                    "Failed to parse state cache: {}. Starting with empty state.",
                    e
                );
                State::default()
            })
        } else {
            State::default()
        };

        Ok(Self {
            cache_path,
            state: Arc::new(RwLock::new(state)),
        })
    }

    #[must_use]
    pub fn get_state(&self) -> Arc<RwLock<State>> {
        self.state.clone()
    }

    pub async fn update_state(&self, new_state: State) -> Result<()> {
        let mut state = self.state.write().await;
        *state = new_state;

        // Save to disk
        let data = serde_json::to_vec(&*state)?;
        drop(state);
        fs::write(&self.cache_path, data)
            .await
            .context("Failed to write state cache to disk")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Route;
    use std::collections::HashMap;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_state_manager_persistence() {
        let temp_file = NamedTempFile::new().unwrap();
        let cache_path = temp_file.path().to_path_buf();

        let mut routes = HashMap::new();
        routes.insert(
            "test.mikrom.local".to_string(),
            Route {
                host: "test.mikrom.local".to_string(),
                targets: vec!["[fd00::1]:8080".to_string()],
            },
        );

        let initial_state = State {
            routes,
            acme_tokens: HashMap::new(),
            certificates: HashMap::new(),
        };

        // 1. Create manager and save state
        {
            let manager = StateManager::new(cache_path.clone()).await.unwrap();
            manager.update_state(initial_state.clone()).await.unwrap();
        }

        // 2. Load manager and verify state
        {
            let manager = StateManager::new(cache_path.clone()).await.unwrap();
            let state_arc = manager.get_state();
            let state = state_arc.read().await;
            assert_eq!(state.routes.len(), 1);
            assert_eq!(
                state.routes.get("test.mikrom.local").unwrap().targets[0],
                "[fd00::1]:8080"
            );
            drop(state);
        }
    }

    #[tokio::test]
    async fn test_state_manager_empty_cache() {
        let temp_file = NamedTempFile::new().unwrap();
        let cache_path = temp_file.path().to_path_buf();
        // Delete the file to ensure it doesn't exist
        std::fs::remove_file(&cache_path).unwrap();

        let manager = StateManager::new(cache_path).await.unwrap();
        let state_arc = manager.get_state();
        let state = state_arc.read().await;
        assert!(state.routes.is_empty());
        assert!(state.acme_tokens.is_empty());
        drop(state);
    }
}
