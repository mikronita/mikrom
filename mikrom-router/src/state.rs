use pingora::lb::LoadBalancer;
use pingora::lb::selection::RoundRobin;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use std::fmt;

#[derive(Debug, Clone, Default)]
pub struct State {
    pub routes: HashMap<String, Route>,
    pub acme_tokens: HashMap<String, String>,
    pub certificates: HashMap<String, Certificate>,
}

#[derive(Clone)]
pub struct Route {
    pub host: String,
    pub targets: Vec<String>,
    pub lb: Arc<LoadBalancer<RoundRobin>>,
}

impl fmt::Debug for Route {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Route")
            .field("host", &self.host)
            .field("targets", &self.targets)
            .field("lb", &"LoadBalancer<RoundRobin>")
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Certificate {
    pub cert_pem: String,
    pub key_pem: String,
}
