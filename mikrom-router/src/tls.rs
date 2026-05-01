use moka::sync::Cache;
use sqlx::PgPool;
use std::io::Cursor;
use std::sync::Arc;
use tokio_rustls::rustls::crypto::ring;
use tokio_rustls::rustls::pki_types::CertificateDer;
use tokio_rustls::rustls::server::{ClientHello, ResolvesServerCert};
use tokio_rustls::rustls::sign::CertifiedKey;
use tracing::{error, info, warn};

#[derive(Debug)]
pub struct DatabaseCertResolver {
    db: PgPool,
    cache: Cache<String, Arc<CertifiedKey>>,
    master_key: String,
}

impl DatabaseCertResolver {
    pub fn new(db: PgPool, master_key: String, cache_ttl: u64) -> Self {
        let cache = Cache::builder()
            .max_capacity(1000)
            .time_to_live(std::time::Duration::from_secs(cache_ttl))
            .build();
        Self {
            db,
            cache,
            master_key,
        }
    }

    pub fn load_cert_from_db(&self, sni: &str) -> Option<Arc<CertifiedKey>> {
        info!("Attempting to load certificate for SNI: '{}'", sni);
        // Use block_in_place to bridge the sync ResolvesServerCert trait with the async PgPool
        tokio::task::block_in_place(|| {
            let rt = tokio::runtime::Handle::current();

            let result = rt.block_on(async {
                sqlx::query(
                    "SELECT cert_chain, private_key FROM tls_certificates WHERE hostname = $1",
                )
                .bind(sni)
                .fetch_optional(&self.db)
                .await
            });

            match result {
                Ok(Some(row)) => {
                    info!("Found certificate in DB for '{}'", sni);
                    let cert_chain_pem: String = row.get("cert_chain");
                    let encrypted_key: String = row.get("private_key");

                    // Decrypt private key
                    let private_key_pem =
                        match crate::crypto::decrypt(&encrypted_key, &self.master_key) {
                            Ok(key) => key,
                            Err(e) => {
                                error!("Failed to decrypt private key for '{}': {}", sni, e);
                                return None;
                            },
                        };

                    match parse_and_sign_cert(&cert_chain_pem, &private_key_pem) {
                        Ok(key) => {
                            let arc_key = Arc::new(key);
                            self.cache.insert(sni.to_string(), arc_key.clone());
                            info!("Successfully parsed and cached certificate for '{}'", sni);
                            Some(arc_key)
                        },
                        Err(e) => {
                            error!("Failed to parse certificate for '{}': {}", sni, e);
                            None
                        },
                    }
                },
                Ok(None) => {
                    warn!("No certificate found in DB for '{}'", sni);
                    None
                },
                Err(e) => {
                    error!("Database error fetching certificate for '{}': {}", sni, e);
                    None
                },
            }
        })
    }
}

use sqlx::Row;

impl ResolvesServerCert for DatabaseCertResolver {
    fn resolve(&self, client_hello: ClientHello) -> Option<Arc<CertifiedKey>> {
        let sni = client_hello.server_name()?;

        if let Some(cert) = self.cache.get(sni) {
            return Some(cert);
        }

        self.load_cert_from_db(sni)
    }
}

fn parse_and_sign_cert(chain_pem: &str, key_pem: &str) -> anyhow::Result<CertifiedKey> {
    let mut cert_reader = Cursor::new(chain_pem);
    let certs: Vec<CertificateDer> = rustls_pemfile::certs(&mut cert_reader)
        .filter_map(Result::ok)
        .collect();

    if certs.is_empty() {
        return Err(anyhow::anyhow!("No certificates found in chain"));
    }

    let mut key_reader = Cursor::new(key_pem);
    let key = rustls_pemfile::private_key(&mut key_reader)?
        .ok_or_else(|| anyhow::anyhow!("No private key found"))?;

    let signing_key = ring::sign::any_supported_type(&key)
        .map_err(|_| anyhow::anyhow!("Unsupported private key type"))?;

    Ok(CertifiedKey::new(certs, signing_key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_and_sign_cert_invalid_data() {
        let result = parse_and_sign_cert("invalid cert", "invalid key");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_and_sign_cert_empty() {
        let result = parse_and_sign_cert("", "");
        assert!(result.is_err());
    }
}
