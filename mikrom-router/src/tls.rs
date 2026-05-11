use crate::state::State;
use async_trait::async_trait;
use openssl::pkey::PKey;
use openssl::x509::X509;
use pingora::listeners::TlsAccept;
use pingora::protocols::tls::TlsRef;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error};

pub struct MikromTlsHandler {
    state: Arc<RwLock<State>>,
}

impl MikromTlsHandler {
    pub const fn new(state: Arc<RwLock<State>>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl TlsAccept for MikromTlsHandler {
    async fn certificate_callback(&self, ssl: &mut TlsRef) {
        let sni = ssl
            .servername(openssl::ssl::NameType::HOST_NAME)
            .unwrap_or("")
            .to_string();
        if sni.is_empty() {
            return;
        }
        debug!("TLS SNI: {sni}");

        let state = self.state.read().await;

        if let Some(cert_info) = state.certificates.get(&sni) {
            // HIGH: Use pre-parsed native types for performance
            if let Some(cert) = &cert_info.parsed_cert {
                if let Err(e) = ssl.set_certificate(cert) {
                    error!("Failed to set certificate for {sni}: {e}");
                    return;
                }
            } else {
                // Fallback to parsing if not pre-parsed (should not happen in prod)
                match X509::from_pem(cert_info.cert_pem.as_bytes()) {
                    Ok(cert) => {
                        if let Err(e) = ssl.set_certificate(&cert) {
                            error!("Failed to set certificate for {sni}: {e}");
                            return;
                        }
                    },
                    Err(e) => {
                        error!("Invalid certificate for {sni}: {e}");
                        return;
                    },
                }
            }

            if let Some(pkey) = &cert_info.parsed_key {
                if let Err(e) = ssl.set_private_key(pkey) {
                    error!("Failed to set private key for {sni}: {e}");
                    return;
                }
            } else {
                // Fallback to parsing if not pre-parsed
                match PKey::private_key_from_pem(cert_info.key_pem.as_bytes()) {
                    Ok(pkey) => {
                        if let Err(e) = ssl.set_private_key(&pkey) {
                            error!("Failed to set private key for {sni}: {e}");
                            return;
                        }
                    },
                    Err(e) => {
                        error!("Invalid private key for {sni}: {e}");
                        return;
                    },
                }
            }
            debug!("Successfully loaded certificate for {sni}");
        }
    }
}
