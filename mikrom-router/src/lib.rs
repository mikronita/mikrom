pub mod config;
pub mod resolver;

pub use resolver::{AppState, resolve_target};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct RouterConfig {
    pub hostname: String,
    pub target_url: Option<String>,
}
