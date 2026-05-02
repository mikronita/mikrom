use anyhow::{Context, Result};
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose,
};
use std::{fs, path::Path};

fn main() -> Result<()> {
    let out_dir_str = std::env::var("CERTS_OUT_DIR").unwrap_or_else(|_| "/certs".to_string());
    let out_dir = Path::new(&out_dir_str);

    // Idempotent: skip generation if the CA cert already exists.
    let ca_pem_path = out_dir.join("ca").join("ca.pem");
    if ca_pem_path.exists() {
        println!("Certificates already exist at '{out_dir_str}', skipping generation.");
        return Ok(());
    }

    println!("Generating mTLS certificates in '{out_dir_str}'...");

    // 1. Generate CA
    let (ca_cert, ca_key) = generate_ca(out_dir)?;

    // 2. Generate Service certs (ServerAuth + ClientAuth)
    let services: &[(&str, &[&str])] = &[
        ("scheduler", &["localhost", "mikrom-scheduler"]),
        ("agent", &["localhost", "mikrom-agent"]),
        ("api", &["localhost", "mikrom-api"]),
    ];

    for (name, sans) in services {
        generate_service_cert(out_dir, name, sans, &ca_cert, &ca_key)?;
    }

    println!("Done.");
    Ok(())
}

/// Generates the root CA certificate and key, saving them to out_dir/ca.
fn generate_ca(out_dir: &Path) -> Result<(Certificate, KeyPair)> {
    let ca_key = KeyPair::generate()?;
    let mut ca_params = CertificateParams::default();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];

    let mut ca_dn = DistinguishedName::new();
    ca_dn.push(DnType::CommonName, "mikrom-ca");
    ca_dn.push(DnType::OrganizationName, "mikrom");
    ca_params.distinguished_name = ca_dn;

    let ca_cert = ca_params.self_signed(&ca_key)?;
    let ca_cert_pem = ca_cert.pem();

    let ca_dir = out_dir.join("ca");
    fs::create_dir_all(&ca_dir).context("Failed to create CA directory")?;
    fs::write(ca_dir.join("ca.pem"), &ca_cert_pem).context("Failed to write CA cert")?;

    println!("  [ca]        → {}", ca_dir.join("ca.pem").display());
    Ok((ca_cert, ca_key))
}

/// Generates a service certificate signed by the CA and saves it to out_dir/name.
fn generate_service_cert(
    out_dir: &Path,
    name: &str,
    sans: &[&str],
    ca_cert: &Certificate,
    ca_key: &KeyPair,
) -> Result<()> {
    let service_key = KeyPair::generate()?;
    let sans_vec: Vec<String> = sans.iter().map(|s| s.to_string()).collect();
    let mut params = CertificateParams::new(sans_vec)?;
    params.extended_key_usages = vec![
        ExtendedKeyUsagePurpose::ServerAuth,
        ExtendedKeyUsagePurpose::ClientAuth,
    ];

    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, name);
    dn.push(DnType::OrganizationName, "mikrom");
    params.distinguished_name = dn;

    let service_cert = params.signed_by(&service_key, ca_cert, ca_key)?;

    let service_dir = out_dir.join(name);
    fs::create_dir_all(&service_dir)
        .with_context(|| format!("Failed to create directory for {name}"))?;
    fs::write(service_dir.join("cert.pem"), service_cert.pem())
        .with_context(|| format!("Failed to write certificate for {name}"))?;
    fs::write(service_dir.join("key.pem"), service_key.serialize_pem())
        .with_context(|| format!("Failed to write key for {name}"))?;

    // Every service gets a copy of the CA cert so it can verify its peers.
    fs::write(service_dir.join("ca.pem"), ca_cert.pem())
        .with_context(|| format!("Failed to write CA copy for {name}"))?;

    println!("  [{name:<9}] → {}", service_dir.display());
    Ok(())
}
