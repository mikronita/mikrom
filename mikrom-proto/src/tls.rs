use rcgen::generate_simple_self_signed;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct TlsConfig {
    pub certificate_pem: String,
    pub private_key_pem: String,
    pub ca_certificate_pem: Option<String>,
}

impl TlsConfig {
    pub fn generate_server_tls(service_name: &str) -> Result<Self, rcgen::Error> {
        let mut subject_alt_names = vec!["localhost".to_string()];
        if !service_name.is_empty() {
            subject_alt_names.push(service_name.to_string());
        }

        let certified_key = generate_simple_self_signed(subject_alt_names)?;

        let cert_pem = certified_key.cert.pem();
        let key_pem = certified_key.key_pair.serialize_pem();

        Ok(Self {
            certificate_pem: cert_pem.clone(),
            private_key_pem: key_pem,
            ca_certificate_pem: Some(cert_pem),
        })
    }

    pub fn load_or_generate(
        service_name: &str,
        cert_path: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let cert_dir = Path::new(cert_path).parent().map(|p| p.to_path_buf());

        if let Some(dir) = cert_dir.clone() {
            if !dir.exists() {
                fs::create_dir_all(&dir)?;
            }
        }

        let cert_file = format!("{}.pem", cert_path);
        let key_file = format!("{}-key.pem", cert_path);

        if Path::new(&cert_file).exists() && Path::new(&key_file).exists() {
            tracing::info!("Loading existing TLS certificates from {}", cert_path);
            return Ok(Self {
                certificate_pem: fs::read_to_string(&cert_file)?,
                private_key_pem: fs::read_to_string(&key_file)?,
                ca_certificate_pem: None,
            });
        }

        tracing::info!("Generating new TLS certificates for {}", service_name);
        let config = Self::generate_server_tls(service_name)?;

        if let Some(dir) = cert_dir {
            let cert_path_out = dir.join("server.pem");
            let key_path_out = dir.join("server-key.pem");
            fs::write(&cert_path_out, &config.certificate_pem)?;
            fs::write(&key_path_out, &config.private_key_pem)?;
            tracing::info!("Certificates saved to {:?}", dir);
        }

        Ok(config)
    }

    pub fn create_server_tls_config(&self) -> Option<tonic::transport::ServerTlsConfig> {
        if self.certificate_pem.is_empty() || self.private_key_pem.is_empty() {
            return None;
        }

        let identity =
            tonic::transport::Identity::from_pem(&self.certificate_pem, &self.private_key_pem);

        Some(tonic::transport::ServerTlsConfig::new().identity(identity))
    }

    pub fn create_client_tls_config(&self) -> Option<tonic::transport::ClientTlsConfig> {
        if self.ca_certificate_pem.is_some() {
            Some(tonic::transport::ClientTlsConfig::new())
        } else {
            None
        }
    }
}
