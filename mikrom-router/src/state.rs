use openssl::pkey::{PKey, Private};
use openssl::x509::X509;
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
    pub use_tls: bool,
    pub tls_alternative_cn: Option<String>,
}

impl fmt::Debug for Route {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Route")
            .field("host", &self.host)
            .field("targets", &self.targets)
            .field("use_tls", &self.use_tls)
            .field("tls_alternative_cn", &self.tls_alternative_cn)
            .field("lb", &"LoadBalancer<RoundRobin>")
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Certificate {
    pub cert_pem: String,
    pub key_pem: String,
    #[serde(skip)]
    pub parsed_chain: Vec<X509>,
    #[serde(skip)]
    pub parsed_key: Option<PKey<Private>>,
}

impl fmt::Debug for Certificate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Certificate")
            .field("cert_pem_len", &self.cert_pem.len())
            .field("key_pem_len", &self.key_pem.len())
            .field("chain_len", &self.parsed_chain.len())
            .field("has_key", &self.parsed_key.is_some())
            .finish_non_exhaustive()
    }
}
