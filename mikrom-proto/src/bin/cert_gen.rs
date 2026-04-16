use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose,
};
use std::{fs, path::Path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = std::env::var("CERTS_OUT_DIR").unwrap_or_else(|_| "/certs".to_string());

    // Idempotent: skip generation if the CA cert already exists.
    // This prevents a cert-gen re-run (e.g. on `docker compose up <single-service>`)
    // from invalidating certs that running services already loaded.
    let ca_pem_path = Path::new(&out_dir).join("ca").join("ca.pem");
    if ca_pem_path.exists() {
        println!("Certificates already exist at '{out_dir}', skipping generation.");
        return Ok(());
    }

    println!("Generating mTLS certificates in '{out_dir}'...");

    // ── 1. CA ────────────────────────────────────────────────────────────────
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

    let ca_dir = Path::new(&out_dir).join("ca");
    fs::create_dir_all(&ca_dir)?;
    fs::write(ca_dir.join("ca.pem"), &ca_cert_pem)?;
    println!("  [ca]        → {}", ca_dir.join("ca.pem").display());

    // ── 2. Service certs (ServerAuth + ClientAuth) ───────────────────────────
    // SANs must include the Docker Compose service name so that TLS name checks
    // pass when services connect to each other by service name.
    let services: &[(&str, &[&str])] = &[
        ("scheduler", &["localhost", "mikrom-scheduler"]),
        ("agent", &["localhost", "mikrom-agent"]),
        ("api", &["localhost", "mikrom-api"]),
    ];

    for (name, sans) in services {
        let service_key = KeyPair::generate()?;
        let sans_vec: Vec<String> = sans.iter().map(|s| s.to_string()).collect();
        let mut params = CertificateParams::new(sans_vec)?;
        params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ];
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, *name);
        dn.push(DnType::OrganizationName, "mikrom");
        params.distinguished_name = dn;

        let service_cert = params.signed_by(&service_key, &ca_cert, &ca_key)?;

        let service_dir = Path::new(&out_dir).join(name);
        fs::create_dir_all(&service_dir)?;
        fs::write(service_dir.join("cert.pem"), service_cert.pem())?;
        fs::write(service_dir.join("key.pem"), service_key.serialize_pem())?;
        // Every service gets a copy of the CA cert so it can verify its peers.
        fs::write(service_dir.join("ca.pem"), &ca_cert_pem)?;

        println!("  [{name:<9}] → {}", service_dir.display());
    }

    println!("Done.");
    Ok(())
}
