use std::fs;
use std::path::Path;

/// Certificates for one service, loaded at runtime from the shared certs volume.
///
/// Expected layout under `dir`:
///   cert.pem  — service certificate (signed by the shared CA)
///   key.pem   — service private key
///   ca.pem    — CA certificate (used to verify peer certs)
#[derive(Clone)]
pub struct ServiceCerts {
    cert_pem: String,
    key_pem: String,
    ca_cert_pem: String,
}

impl ServiceCerts {
    pub fn load(dir: &str) -> Result<Self, std::io::Error> {
        let dir = Path::new(dir);
        Ok(Self {
            cert_pem: fs::read_to_string(dir.join("cert.pem"))?,
            key_pem: fs::read_to_string(dir.join("key.pem"))?,
            ca_cert_pem: fs::read_to_string(dir.join("ca.pem"))?,
        })
    }

    /// mTLS server config: presents our cert + requires a client cert signed by the shared CA.
    pub fn server_tls_config(
        &self,
    ) -> Result<tonic::transport::ServerTlsConfig, tonic::transport::Error> {
        let identity =
            tonic::transport::Identity::from_pem(&self.cert_pem, &self.key_pem);
        let ca = tonic::transport::Certificate::from_pem(&self.ca_cert_pem);
        Ok(tonic::transport::ServerTlsConfig::new()
            .identity(identity)
            .client_ca_root(ca))
    }

    /// mTLS client config: presents our cert + verifies server cert against the shared CA.
    ///
    /// `server_domain` must match a SAN in the server's certificate
    /// (e.g. "mikrom-scheduler", "mikrom-agent").
    pub fn client_tls_config(&self, server_domain: &str) -> tonic::transport::ClientTlsConfig {
        let identity =
            tonic::transport::Identity::from_pem(&self.cert_pem, &self.key_pem);
        let ca = tonic::transport::Certificate::from_pem(&self.ca_cert_pem);
        tonic::transport::ClientTlsConfig::new()
            .domain_name(server_domain)
            .ca_certificate(ca)
            .identity(identity)
    }
}
