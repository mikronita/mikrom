use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use chrono::{DateTime, Utc};
use instant_acme::{
    Account, ChallengeType, Identifier, LetsEncrypt, NewAccount, NewOrder, OrderStatus,
};
use sqlx::{PgPool, Row};
use std::time::Duration;
use tracing::{error, info, warn};

pub async fn acme_challenge_handler(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> impl IntoResponse {
    let result = sqlx::query("SELECT key_auth FROM acme_challenges WHERE token = $1")
        .bind(&token)
        .fetch_optional(&state.db)
        .await;

    match result {
        Ok(Some(row)) => {
            let key_auth: String = row.get("key_auth");
            info!("Serving ACME challenge for token: {}", token);
            key_auth.into_response()
        },
        Ok(None) => {
            warn!("ACME challenge token not found: {}", token);
            StatusCode::NOT_FOUND.into_response()
        },
        Err(e) => {
            error!("Database error serving ACME challenge: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        },
    }
}

pub async fn start_acme_worker(db: PgPool, email: String, staging: bool) {
    info!("Starting ACME worker (staging: {})", staging);

    let url = if staging {
        LetsEncrypt::Staging.url()
    } else {
        LetsEncrypt::Production.url()
    };

    loop {
        if let Err(e) = run_acme_iteration(&db, &email, url).await {
            error!("ACME iteration failed: {}", e);
        }
        tokio::time::sleep(Duration::from_secs(3600)).await; // Check every hour
    }
}

async fn run_acme_iteration(db: &PgPool, email: &str, acme_url: &str) -> anyhow::Result<()> {
    // 1. Find domains that need certificates
    let domains_to_certify = sqlx::query(
        r#"
        SELECT r.hostname 
        FROM routes r
        LEFT JOIN tls_certificates c ON r.hostname = c.hostname
        WHERE c.hostname IS NULL OR c.expires_at < NOW() + INTERVAL '30 days'
        "#,
    )
    .fetch_all(db)
    .await?;

    for row in domains_to_certify {
        let hostname: String = row.get("hostname");
        info!("Processing certificate for {}", hostname);

        if let Err(e) = certify_domain(db, email, acme_url, &hostname).await {
            error!("Failed to certify domain {}: {}", hostname, e);
        }
    }

    Ok(())
}

async fn certify_domain(
    db: &PgPool,
    email: &str,
    acme_url: &str,
    hostname: &str,
) -> anyhow::Result<()> {
    let contact_url = format!("mailto:{}", email);
    let contacts = [contact_url.as_str()];

    let (account, _) = Account::builder()?
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

    let mut order = account
        .new_order(&NewOrder::new(&[Identifier::Dns(hostname.to_string())]))
        .await?;

    // In 0.8.5, authorizations() returns an Authorizations struct (iterator-like)
    let mut auths = order.authorizations();

    while let Some(auth_result) = auths.next().await {
        let mut auth_handle = auth_result?;
        if let Some(mut challenge_handle) = auth_handle.challenge(ChallengeType::Http01) {
            let key_auth = challenge_handle.key_authorization().as_str().to_string();
            let token = challenge_handle.token.clone();

            // Save challenge to DB with hostname for cascading delete
            sqlx::query("INSERT INTO acme_challenges (token, key_auth, hostname) VALUES ($1, $2, $3) ON CONFLICT (token) DO UPDATE SET key_auth = EXCLUDED.key_auth, hostname = EXCLUDED.hostname")
                .bind(&token)
                .bind(&key_auth)
                .bind(hostname)
                .execute(db)
                .await?;

            // Trigger challenge
            challenge_handle.set_ready().await?;
        }
    }

    // Wait for order to be ready
    let mut state = order.state();
    while matches!(
        state.status,
        OrderStatus::Pending | OrderStatus::Processing | OrderStatus::Ready
    ) {
        if state.status == OrderStatus::Ready {
            break;
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
        state = order.refresh().await?;
    }

    if state.status != OrderStatus::Ready {
        return Err(anyhow::anyhow!(
            "ACME order failed with status: {:?}",
            state.status
        ));
    }

    // Finalize order - 0.8.5 finalize() with 'rcgen' feature generates CSR and returns private key PEM
    let private_key_pem = order.finalize().await?;

    // Wait for valid status
    let mut state = order.refresh().await?;
    while state.status == OrderStatus::Processing {
        tokio::time::sleep(Duration::from_secs(2)).await;
        state = order.refresh().await?;
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

    // Save to DB
    sqlx::query("INSERT INTO tls_certificates (hostname, cert_chain, private_key, expires_at) VALUES ($1, $2, $3, $4) ON CONFLICT (hostname) DO UPDATE SET cert_chain = EXCLUDED.cert_chain, private_key = EXCLUDED.private_key, expires_at = EXCLUDED.expires_at, updated_at = NOW()")
        .bind(hostname)
        .bind(&cert_chain_pem)
        .bind(&private_key_pem)
        .bind(expires_at)
        .execute(db)
        .await?;

    info!("Successfully certified domain: {}", hostname);

    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppState;
    use axum::extract::Path;
    use hyper_util::client::legacy::connect::HttpConnector;
    use hyper_util::rt::TokioExecutor;
    use moka::future::Cache;

    #[tokio::test]
    async fn test_acme_challenge_handler_not_found() {
        let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgres://mikrom:mikrom_password@localhost:5432/mikrom_router".to_string()
        });
        let db = PgPool::connect(&db_url).await.unwrap();

        let state = AppState {
            db,
            cache: Cache::builder().build(),
            client: hyper_util::client::legacy::Client::builder(TokioExecutor::new())
                .build(HttpConnector::new()),
        };

        let response = acme_challenge_handler(State(state), Path("non-existent-token".to_string()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
