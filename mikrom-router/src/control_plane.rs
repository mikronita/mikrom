use crate::state::{Certificate, Route, State};
use crate::state_manager::StateManager;
use anyhow::Result;
use async_nats::Client;
use futures_util::StreamExt;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, info};

pub struct ControlPlane {
    db: PgPool,
    nats: Client,
    state_manager: Arc<StateManager>,
}

impl ControlPlane {
    #[must_use]
    pub const fn new(db: PgPool, nats: Client, state_manager: Arc<StateManager>) -> Self {
        Self {
            db,
            nats,
            state_manager,
        }
    }

    pub async fn run(&self) -> Result<()> {
        // 1. Initial sync
        self.sync_full_state().await?;

        // 2. Listen for updates via NATS (Simplified for now)
        let mut subscriber = self.nats.subscribe("mikrom.router.>").await?;

        info!("Control plane listening for NATS updates...");
        while let Some(msg) = subscriber.next().await {
            info!("Received NATS update: {:?}", msg.subject);
            if let Err(e) = self.sync_full_state().await {
                error!("Failed to sync state after NATS update: {}", e);
            }
        }

        Ok(())
    }

    async fn sync_full_state(&self) -> Result<()> {
        info!("Performing full state sync from database...");

        // Fetch routes
        let route_rows = sqlx::query(
            r"
            SELECT a.name as app_name, a.custom_domain, d.ipv6 as target_ip
            FROM apps a
            JOIN deployments d ON a.active_deployment_id = d.id
            WHERE d.status = 'running'
            ",
        )
        .fetch_all(&self.db)
        .await?;

        let mut routes: HashMap<String, Route> = HashMap::new();
        for row in route_rows {
            use sqlx::Row;
            let app_name: String = row.get("app_name");
            let custom_domain: Option<String> = row.get("custom_domain");
            let target_ip: String = row.get("target_ip");

            let target = format!("[{target_ip}]:8080");

            // 1. Internal route
            let internal_host = format!("{app_name}.mikrom.local");
            routes
                .entry(internal_host.clone())
                .and_modify(|r| r.targets.push(target.clone()))
                .or_insert_with(|| Route {
                    host: internal_host,
                    targets: vec![target.clone()],
                });

            // 2. Custom domain route
            if let Some(domain) = custom_domain {
                routes
                    .entry(domain.clone())
                    .and_modify(|r| r.targets.push(target.clone()))
                    .or_insert_with(|| Route {
                        host: domain,
                        targets: vec![target.clone()],
                    });
            }
        }

        // Fetch ACME tokens
        let acme_rows = sqlx::query("SELECT token, key_auth FROM acme_challenges")
            .fetch_all(&self.db)
            .await?;

        let mut acme_tokens = HashMap::new();
        for row in acme_rows {
            use sqlx::Row;
            let token: String = row.get("token");
            let key_auth: String = row.get("key_auth");
            acme_tokens.insert(token, key_auth);
        }

        // Fetch Certificates
        let cert_rows =
            sqlx::query("SELECT hostname, cert_chain, private_key FROM tls_certificates")
                .fetch_all(&self.db)
                .await?;

        let mut certificates = HashMap::new();
        for row in cert_rows {
            use sqlx::Row;
            let domain: String = row.get("hostname");
            let cert_pem: String = row.get("cert_chain");
            let key_pem: String = row.get("private_key");
            certificates.insert(domain, Certificate { cert_pem, key_pem });
        }

        let new_state = State {
            routes,
            acme_tokens,
            certificates,
        };

        self.state_manager.update_state(new_state).await?;
        info!("State sync complete.");

        Ok(())
    }
}
