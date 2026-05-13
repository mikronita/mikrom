use chrono::{DateTime, Utc};
use instant_acme::{
    Account, ChallengeType, Identifier, LetsEncrypt, NewAccount, NewOrder, OrderStatus,
};
use mikrom_proto::router::{AcmeChallengeUpdate, TlsCertificateUpdate};
use mikrom_proto::subjects;
use sqlx::{PgPool, Row};
use std::time::Duration;
use tracing::{error, info};

pub async fn start_acme_worker(
    api_db: PgPool,
    nats: crate::nats::TypedNatsClient,
    email: String,
    staging: bool,
    master_key: String,
    interval_secs: u64,
    router_addr: String,
) {
    info!(
        "Starting ACME worker (staging: {}, email: {}, router: {})",
        staging, email, router_addr
    );

    let url = if staging {
        LetsEncrypt::Staging.url()
    } else {
        LetsEncrypt::Production.url()
    };

    loop {
        if let Err(e) = run_acme_iteration(
            &api_db,
            &nats,
            &email,
            url,
            staging,
            &master_key,
            &router_addr,
        )
        .await
        {
            error!("ACME iteration failed: {}", e);
        }
        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
    }
}

pub async fn trigger_domain_certification(
    state: &crate::AppState,
    hostname: &str,
) -> anyhow::Result<()> {
    let url = if state.acme_staging {
        LetsEncrypt::Staging.url()
    } else {
        LetsEncrypt::Production.url()
    };

    // Check if it needs certification first to avoid redundant work
    let needs_cert = sqlx::query(
        "SELECT 1 FROM apps WHERE hostname = $1 AND (cert_expires_at IS NULL OR cert_expires_at < NOW() + INTERVAL '30 days')"
    )
    .bind(hostname)
    .fetch_optional(&state.api_db)
    .await?
    .is_some();

    if !needs_cert {
        return Ok(());
    }

    info!("Triggering immediate certificate issuance for {}", hostname);

    let account =
        get_or_create_acme_account(&state.api_db, &state.acme_email, url, state.acme_staging)
            .await?;

    certify_domain(
        &state.api_db,
        &state.nats,
        &account,
        hostname,
        &state.master_key,
        &state.router_addr,
    )
    .await
}

pub async fn run_acme_iteration(
    api_db: &PgPool,
    nats: &crate::nats::TypedNatsClient,
    email: &str,
    acme_url: &str,
    is_staging: bool,
    master_key: &str,
    router_addr: &str,
) -> anyhow::Result<()> {
    // 1. Find domains that need certificates (expired or expiring in < 30 days)
    let domains_to_certify = sqlx::query(
        r#"
        SELECT hostname
        FROM apps
        WHERE hostname IS NOT NULL
          AND (cert_expires_at IS NULL OR cert_expires_at < NOW() + INTERVAL '30 days')
        "#,
    )
    .fetch_all(api_db)
    .await?;

    if domains_to_certify.is_empty() {
        return Ok(());
    }

    // 2. Get or create ACME account from the API database
    let account = get_or_create_acme_account(api_db, email, acme_url, is_staging).await?;

    for row in domains_to_certify {
        let hostname: String = row.get("hostname");

        info!("Processing certificate renewal for {}", hostname);

        if let Err(e) =
            certify_domain(api_db, nats, &account, &hostname, master_key, router_addr).await
        {
            error!("Failed to certify domain {}: {}", hostname, e);
        }
    }

    Ok(())
}

pub async fn get_or_create_acme_account(
    api_db: &PgPool,
    email: &str,
    acme_url: &str,
    is_staging: bool,
) -> anyhow::Result<Account> {
    let row = sqlx::query(
        "SELECT credentials_json FROM acme_accounts WHERE email = $1 AND is_staging = $2",
    )
    .bind(email)
    .bind(is_staging)
    .fetch_optional(api_db)
    .await?;

    if let Some(row) = row {
        let credentials_json: String = row.get("credentials_json");
        let credentials = serde_json::from_str(&credentials_json)?;

        info!("Using existing ACME account for {}", email);
        return Ok(Account::builder()?.from_credentials(credentials).await?);
    }

    info!("Creating new ACME account for {}", email);
    let contact_url = format!("mailto:{}", email);
    let contacts = [contact_url.as_str()];

    let (account, credentials) = Account::builder()?
        .create(
            &NewAccount {
                contact: &contacts,
                terms_of_service_agreed: true,
                only_return_existing: false,
            },
            acme_url.to_string(),
            None,
        )
        .await?;

    let credentials_json = serde_json::to_string(&credentials)?;

    sqlx::query(
        "INSERT INTO acme_accounts (email, credentials_json, is_staging) VALUES ($1, $2, $3)",
    )
    .bind(email)
    .bind(&credentials_json)
    .bind(is_staging)
    .execute(api_db)
    .await?;

    Ok(account)
}

async fn certify_domain(
    api_db: &PgPool,
    nats: &crate::nats::TypedNatsClient,
    account: &Account,
    hostname: &str,
    master_key: &str,
    router_addr: &str,
) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()?;

    let mut order = account
        .new_order(&NewOrder::new(&[Identifier::Dns(hostname.to_string())]))
        .await?;

    let mut auths = order.authorizations();
    let mut tokens = Vec::new();

    while let Some(auth_result) = auths.next().await {
        let mut auth_handle = auth_result?;
        if let Some(mut challenge_handle) = auth_handle.challenge(ChallengeType::Http01) {
            let key_auth = challenge_handle.key_authorization().as_str().to_string();
            let token = challenge_handle.token.clone();
            tokens.push(token.clone());

            // Publish challenge to NATS
            let update = AcmeChallengeUpdate {
                token: token.clone(),
                key_auth: key_auth.clone(),
                hostname: hostname.to_string(),
                is_delete: false,
            };

            nats.publish(subjects::ROUTER_ACME_CHALLENGE_UPDATED, update)
                .await?;

            // Wait for router to receive and apply challenge (eventual consistency)
            verify_challenge_is_live(&client, hostname, &token, &key_auth, router_addr).await?;

            // Trigger challenge
            challenge_handle.set_ready().await?;
        }
    }

    // Wait for order to be ready
    let mut state = order.state();
    let mut attempts = 0;
    while matches!(
        state.status,
        OrderStatus::Pending | OrderStatus::Processing | OrderStatus::Ready
    ) && attempts < 12
    {
        if state.status == OrderStatus::Ready {
            break;
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
        state = order.refresh().await?;
        attempts += 1;
    }

    // Cleanup challenges from router
    for token in tokens {
        let update = AcmeChallengeUpdate {
            token,
            key_auth: "".into(),
            hostname: "".into(),
            is_delete: true,
        };
        if let Err(e) = nats
            .publish(subjects::ROUTER_ACME_CHALLENGE_UPDATED, update)
            .await
        {
            error!(
                "Failed to publish challenge cleanup for {}: {}",
                hostname, e
            );
        }
    }

    if state.status != OrderStatus::Ready {
        return Err(anyhow::anyhow!(
            "ACME order failed with status: {:?} after {} attempts",
            state.status,
            attempts
        ));
    }

    // Finalize order
    let private_key_pem = order.finalize().await?;

    // Wait for valid status
    let mut state = order.refresh().await?;
    attempts = 0;
    while state.status == OrderStatus::Processing && attempts < 10 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        state = order.refresh().await?;
        attempts += 1;
    }

    if state.status != OrderStatus::Valid {
        return Err(anyhow::anyhow!(
            "ACME order finalization failed with status: {:?}",
            state.status
        ));
    }

    // Download certificate
    let cert_chain_pem = order
        .certificate()
        .await?
        .ok_or_else(|| anyhow::anyhow!("No certificate returned"))?;

    // Parse expiry
    let expires_at = parse_expiry(&cert_chain_pem)?;

    // Update expiry in API database
    sqlx::query("UPDATE apps SET cert_expires_at = $1 WHERE hostname = $2")
        .bind(expires_at)
        .bind(hostname)
        .execute(api_db)
        .await?;

    // Encrypt private key for storage
    let encrypted_key = crate::crypto::encrypt(&private_key_pem, master_key)
        .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

    // Publish certificate to NATS
    let update = TlsCertificateUpdate {
        hostname: hostname.to_string(),
        cert_chain: cert_chain_pem,
        private_key: encrypted_key,
        expires_at: expires_at.timestamp(),
    };

    nats.publish(subjects::ROUTER_TLS_CERT_UPDATED, update)
        .await?;

    info!(
        "Successfully certified domain and published to NATS: {}",
        hostname
    );

    Ok(())
}

async fn verify_challenge_is_live(
    client: &reqwest::Client,
    hostname: &str,
    token: &str,
    expected_auth: &str,
    router_addr: &str,
) -> anyhow::Result<()> {
    let url = format!("{}/.well-known/acme-challenge/{}", router_addr, token);

    info!(
        "Verifying ACME challenge is live: {} (via Host: {})",
        url, hostname
    );

    let mut attempts = 0;
    while attempts < 20 {
        match client.get(&url).header("Host", hostname).send().await {
            Ok(res) => {
                if res.status().is_success() {
                    let body = res.text().await?;
                    if body.trim() == expected_auth {
                        info!("ACME challenge verified successfully on router.");
                        return Ok(());
                    } else {
                        tracing::warn!(
                            "ACME challenge body mismatch: expected '{}', got '{}'",
                            expected_auth,
                            body
                        );
                    }
                } else {
                    tracing::debug!("ACME challenge not ready yet (status: {})", res.status());
                }
            },
            Err(e) => {
                tracing::debug!("Failed to connect to router for verification: {}", e);
            },
        }
        attempts += 1;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    Err(anyhow::anyhow!(
        "ACME challenge verification timed out for {}",
        hostname
    ))
}

fn parse_expiry(cert_pem: &str) -> anyhow::Result<DateTime<Utc>> {
    let (_, pem) = x509_parser::pem::parse_x509_pem(cert_pem.as_bytes())
        .map_err(|e| anyhow::anyhow!("Failed to parse PEM: {}", e))?;
    let x509 = pem
        .parse_x509()
        .map_err(|e| anyhow::anyhow!("Failed to parse X509: {}", e))?;

    let not_after = x509.validity().not_after;
    let timestamp = not_after.timestamp();

    Ok(DateTime::from_timestamp(timestamp, 0).unwrap_or(Utc::now()))
}
