use anyhow::{Context, Result};
use openssl::x509::X509;
use std::path::PathBuf;
use std::sync::Arc;

pub fn load_upstream_ca(ca_certs_dir: Option<&str>) -> Result<Option<Arc<Box<[X509]>>>> {
    let Some(ca_certs_dir) = ca_certs_dir else {
        return Ok(None);
    };

    let ca_path = PathBuf::from(ca_certs_dir).join("ca.pem");
    if !ca_path.exists() {
        anyhow::bail!(
            "Upstream CA directory was configured but {} does not exist",
            ca_path.display()
        );
    }

    let ca_pem = std::fs::read(&ca_path)
        .with_context(|| format!("Failed to read upstream CA from {}", ca_path.display()))?;
    let certs = X509::stack_from_pem(&ca_pem)
        .with_context(|| format!("Failed to parse upstream CA from {}", ca_path.display()))?;

    if certs.is_empty() {
        anyhow::bail!(
            "Upstream CA file {} is empty or contains no certificates",
            ca_path.display()
        );
    }

    Ok(Some(Arc::new(certs.into_boxed_slice())))
}

#[cfg(test)]
mod tests {
    use super::load_upstream_ca;
    use openssl::asn1::Asn1Time;
    use openssl::bn::BigNum;
    use openssl::hash::MessageDigest;
    use openssl::pkey::PKey;
    use openssl::rsa::Rsa;
    use openssl::x509::X509NameBuilder;
    use tempfile::tempdir;

    #[test]
    fn returns_none_when_no_dir_is_configured() {
        assert!(load_upstream_ca(None).unwrap().is_none());
    }

    #[test]
    fn loads_explicit_ca_file() {
        let dir = tempdir().unwrap();
        let ca_path = dir.path().join("ca.pem");

        let rsa = Rsa::generate(2048).unwrap();
        let pkey = PKey::from_rsa(rsa).unwrap();
        let mut name = X509NameBuilder::new().unwrap();
        name.append_entry_by_text("CN", "mikrom-test-ca").unwrap();
        let name = name.build();

        let mut builder = openssl::x509::X509Builder::new().unwrap();
        builder.set_version(2).unwrap();
        let serial = BigNum::from_u32(1).unwrap().to_asn1_integer().unwrap();
        builder.set_serial_number(&serial).unwrap();
        builder.set_subject_name(&name).unwrap();
        builder.set_issuer_name(&name).unwrap();
        builder.set_pubkey(&pkey).unwrap();
        builder
            .set_not_before(&Asn1Time::days_from_now(0).unwrap())
            .unwrap();
        builder
            .set_not_after(&Asn1Time::days_from_now(1).unwrap())
            .unwrap();
        builder.sign(&pkey, MessageDigest::sha256()).unwrap();
        let cert = builder.build();

        std::fs::write(&ca_path, cert.to_pem().unwrap()).unwrap();

        let loaded = load_upstream_ca(Some(dir.path().to_str().unwrap())).unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().len(), 1);
    }

    #[test]
    fn fails_when_explicit_ca_dir_is_missing_file() {
        let dir = tempdir().unwrap();
        let err = load_upstream_ca(Some(dir.path().to_str().unwrap())).unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn fails_when_explicit_ca_file_is_empty() {
        let dir = tempdir().unwrap();
        let ca_path = dir.path().join("ca.pem");
        std::fs::write(&ca_path, b"").unwrap();

        let err = load_upstream_ca(Some(dir.path().to_str().unwrap())).unwrap_err();
        assert!(
            err.to_string()
                .contains("empty or contains no certificates")
        );
    }
}
