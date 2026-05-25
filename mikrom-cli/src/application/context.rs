use crate::application::ports::ApiClient;
use crate::config::Config;
use std::sync::Arc;

pub struct CliContext {
    pub config: Arc<Config>,
    pub client: Arc<dyn ApiClient>,
}

impl CliContext {
    pub fn new(config: Arc<Config>, client: Arc<dyn ApiClient>) -> Self {
        Self { config, client }
    }
}
