use anyhow::{Context, Result};
use async_nats::Client;
use std::path::PathBuf;

fn tls_url(nats_url: &str) -> String {
    nats_url
        .strip_prefix("nats://")
        .map_or_else(|| nats_url.to_string(), |rest| format!("tls://{rest}"))
}

pub async fn connect_nats(
    nats_url: &str,
    use_tls: bool,
    certs_dir: Option<&str>,
) -> Result<Client> {
    if !use_tls {
        return async_nats::connect(nats_url)
            .await
            .with_context(|| format!("Failed to connect to NATS at {nats_url}"));
    }

    let certs_dir =
        certs_dir.context("NATS TLS is enabled but no certs directory was configured")?;
    let certs_dir = PathBuf::from(certs_dir);

    let client = async_nats::ConnectOptions::new()
        .require_tls(true)
        .add_root_certificates(certs_dir.join("ca.pem"))
        .add_client_certificate(certs_dir.join("cert.pem"), certs_dir.join("key.pem"))
        .connect(tls_url(nats_url))
        .await
        .with_context(|| format!("Failed to connect to NATS at {nats_url} with mTLS"))?;

    Ok(client)
}
