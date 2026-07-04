use anyhow::{Context, Result};
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
    /// Loads certificates from the specified directory.
    pub fn load<P: AsRef<Path>>(dir: P) -> Result<Self> {
        let dir = dir.as_ref();
        Ok(Self {
            cert_pem: fs::read_to_string(dir.join("cert.pem"))
                .with_context(|| format!("Failed to read cert.pem from {}", dir.display()))?,
            key_pem: fs::read_to_string(dir.join("key.pem"))
                .with_context(|| format!("Failed to read key.pem from {}", dir.display()))?,
            ca_cert_pem: fs::read_to_string(dir.join("ca.pem"))
                .with_context(|| format!("Failed to read ca.pem from {}", dir.display()))?,
        })
    }

    /// mTLS server config: presents our cert + requires a client cert signed by the shared CA.
    pub fn server_tls_config(&self) -> Result<tonic::transport::ServerTlsConfig> {
        let identity = tonic::transport::Identity::from_pem(&self.cert_pem, &self.key_pem);
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
        let identity = tonic::transport::Identity::from_pem(&self.cert_pem, &self.key_pem);
        let ca = tonic::transport::Certificate::from_pem(&self.ca_cert_pem);
        tonic::transport::ClientTlsConfig::new()
            .domain_name(server_domain)
            .ca_certificate(ca)
            .identity(identity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::{BasicConstraints, CertificateParams, IsCa, KeyPair};
    use std::fs;
    use std::path::Path;

    /// Write a self-signed CA + a leaf cert signed by that CA into `dir`.
    fn write_test_certs(dir: &Path) {
        let ca_key = KeyPair::generate().unwrap();
        let mut ca_params = CertificateParams::default();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca_cert = ca_params.self_signed(&ca_key).unwrap();
        let ca_pem = ca_cert.pem();

        let leaf_key = KeyPair::generate().unwrap();
        let leaf_params = CertificateParams::new(vec!["localhost".to_string()]).unwrap();
        let issuer = rcgen::Issuer::from_ca_cert_der(ca_cert.der(), &ca_key).unwrap();
        let leaf_cert = leaf_params.signed_by(&leaf_key, &issuer).unwrap();

        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("cert.pem"), leaf_cert.pem()).unwrap();
        fs::write(dir.join("key.pem"), leaf_key.serialize_pem()).unwrap();
        fs::write(dir.join("ca.pem"), ca_pem).unwrap();
    }

    fn test_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("mikrom-tls-test-{}", name))
    }

    #[test]
    fn test_load_valid_certs_succeeds() {
        let dir = test_dir("load-valid");
        write_test_certs(&dir);
        assert!(ServiceCerts::load(&dir).is_ok());
    }

    #[test]
    fn test_load_nonexistent_dir_returns_error() {
        let result = ServiceCerts::load("/no/such/path/mikrom-test-xyz");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_missing_cert_pem_returns_error() {
        let dir = test_dir("missing-cert");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("key.pem"), "key").unwrap();
        fs::write(dir.join("ca.pem"), "ca").unwrap();
        assert!(ServiceCerts::load(&dir).is_err());
    }

    #[test]
    fn test_load_missing_key_pem_returns_error() {
        let dir = test_dir("missing-key");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("cert.pem"), "cert").unwrap();
        fs::write(dir.join("ca.pem"), "ca").unwrap();
        assert!(ServiceCerts::load(&dir).is_err());
    }

    #[test]
    fn test_load_missing_ca_pem_returns_error() {
        let dir = test_dir("missing-ca");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("cert.pem"), "cert").unwrap();
        fs::write(dir.join("key.pem"), "key").unwrap();
        assert!(ServiceCerts::load(&dir).is_err());
    }

    #[test]
    fn test_server_tls_config_builds_successfully() {
        let dir = test_dir("server-cfg");
        write_test_certs(&dir);
        let certs = ServiceCerts::load(&dir).unwrap();
        assert!(certs.server_tls_config().is_ok());
    }

    #[test]
    fn test_client_tls_config_builds_without_panic() {
        let dir = test_dir("client-cfg");
        write_test_certs(&dir);
        let certs = ServiceCerts::load(&dir).unwrap();
        let _config = certs.client_tls_config("mikrom-scheduler");
    }

    #[test]
    fn test_client_tls_config_accepts_various_domain_names() {
        let dir = test_dir("client-domains");
        write_test_certs(&dir);
        let certs = ServiceCerts::load(&dir).unwrap();
        let _a = certs.client_tls_config("mikrom-scheduler");
        let _b = certs.client_tls_config("mikrom-agent");
        let _c = certs.client_tls_config("localhost");
        let _d = certs.client_tls_config("");
    }

    #[test]
    fn test_clone_both_produce_valid_server_config() {
        let dir = test_dir("clone-server");
        write_test_certs(&dir);
        let original = ServiceCerts::load(&dir).unwrap();
        let cloned = original.clone();
        assert!(original.server_tls_config().is_ok());
        assert!(cloned.server_tls_config().is_ok());
    }

    #[test]
    fn test_clone_both_produce_valid_client_config() {
        let dir = test_dir("clone-client");
        write_test_certs(&dir);
        let original = ServiceCerts::load(&dir).unwrap();
        let cloned = original.clone();
        let _c1 = original.client_tls_config("host-a");
        let _c2 = cloned.client_tls_config("host-b");
    }
}
