use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct State {
    pub routes: HashMap<String, Route>,
    pub acme_tokens: HashMap<String, String>,
    pub certificates: HashMap<String, Certificate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub host: String,
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Certificate {
    pub cert_pem: String,
    pub key_pem: String,
}
