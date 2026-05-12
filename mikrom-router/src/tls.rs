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
            // 1. Set Leaf Certificate and Chain
            let mut chain_iter = cert_info.parsed_chain.iter();
            if let Some(leaf) = chain_iter.next() {
                if let Err(e) = ssl.set_certificate(leaf) {
                    error!("Failed to set certificate for {sni}: {e}");
                    return;
                }

                // Add remaining certs to chain
                for cert in chain_iter {
                    if let Err(e) = ssl.add_chain_cert(cert.clone()) {
                        error!("Failed to add intermediate cert to chain for {sni}: {e}");
                    }
                }
            } else {
                // Fallback to parsing if not pre-parsed
                match X509::stack_from_pem(cert_info.cert_pem.as_bytes()) {
                    Ok(chain) => {
                        let mut iter = chain.into_iter();
                        if let Some(leaf) = iter.next() {
                            if let Err(e) = ssl.set_certificate(&leaf) {
                                error!("Failed to set certificate for {sni}: {e}");
                                return;
                            }
                            for cert in iter {
                                if let Err(e) = ssl.add_chain_cert(cert) {
                                    error!(
                                        "Failed to add intermediate cert to chain for {sni}: {e}"
                                    );
                                }
                            }
                        }
                    },
                    Err(e) => {
                        error!("Invalid certificate for {sni}: {e}");
                        return;
                    },
                }
            }

            // 2. Set Private Key
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
            debug!("Successfully loaded certificate and chain for {sni}");
        } else {
            error!("No certificate found for SNI: {sni}. This will cause TLS handshake failure.");
        }
    }
}
