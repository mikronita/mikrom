use crate::app::config::{NatsUrl, RouterId};
use crate::app::runtime;
use async_trait::async_trait;
use mikrom_proto::router::RouterTrafficEvent;
use mikrom_proto::subjects;
use pingora::server::ShutdownWatch;
use pingora::services::background::BackgroundService;
use prost::Message;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tracing::{error, info, warn};

use dashmap::DashMap;

pub struct RouterTrafficPublisher {
    router_id: RouterId,
    tx: mpsc::Sender<RouterTrafficEvent>,
    last_sent: DashMap<String, i64>,
}

impl RouterTrafficPublisher {
    #[must_use]
    pub fn new(router_id: RouterId, tx: mpsc::Sender<RouterTrafficEvent>) -> Self {
        Self {
            router_id,
            tx,
            last_sent: DashMap::new(),
        }
    }

    pub fn record(&self, hostname: String) {
        let now = chrono::Utc::now().timestamp();

        // Simple deduplication: 1 event per host every 30 seconds
        if let Some(last) = self.last_sent.get(&hostname)
            && now - *last < 30
        {
            return;
        }

        let hostname_for_event = hostname.clone();
        let event = RouterTrafficEvent {
            hostname,
            router_id: self.router_id.as_str().to_string(),
            timestamp: now,
        };

        if let Err(e) = self.tx.try_send(event) {
            warn!("Router traffic queue is full or closed: {e}");
        } else {
            self.last_sent.insert(hostname_for_event, now);
        }
    }
}

pub struct RouterTrafficLoop {
    nats_url: NatsUrl,
    nats_use_tls: bool,
    nats_certs_dir: Option<String>,
    rx: Arc<Mutex<Option<mpsc::Receiver<RouterTrafficEvent>>>>,
    startup_connect_timeout: std::time::Duration,
}

impl RouterTrafficLoop {
    #[must_use]
    pub fn new(
        nats_url: NatsUrl,
        nats_use_tls: bool,
        nats_certs_dir: Option<String>,
        rx: mpsc::Receiver<RouterTrafficEvent>,
        startup_connect_timeout: std::time::Duration,
    ) -> Self {
        Self {
            nats_url,
            nats_use_tls,
            nats_certs_dir,
            rx: Arc::new(Mutex::new(Some(rx))),
            startup_connect_timeout,
        }
    }
}

#[async_trait]
impl BackgroundService for RouterTrafficLoop {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        runtime::init_tracing_once("router-traffic");

        let nats = runtime::connect_with_backoff(
            "Router traffic loop NATS",
            self.startup_connect_timeout,
            || async {
                crate::infrastructure::nats::connect_nats(
                    self.nats_url.as_str(),
                    self.nats_use_tls,
                    self.nats_certs_dir.as_deref(),
                )
                .await
            },
        )
        .await;
        info!("Router traffic loop: connected to NATS.");

        let Some(mut rx) = self.rx.lock().await.take() else {
            warn!("Router traffic loop started without a receiver");
            return;
        };

        info!("Router traffic loop started");

        loop {
            tokio::select! {
                Some(event) = rx.recv() => {
                    let mut buf = Vec::new();
                    if let Err(e) = event.encode(&mut buf) {
                        error!("Router traffic loop: failed to encode event: {e}");
                        continue;
                    }

                    if let Err(e) = nats.publish(subjects::ROUTER_TRAFFIC_EVENT, buf.into()).await {
                        error!("Router traffic loop: failed to publish event: {e}");
                    }
                }
                _ = shutdown.changed() => {
                    info!("Router traffic loop: shutting down...");
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::timeout;

    #[tokio::test]
    async fn test_record_enqueues_router_traffic_event() {
        let (tx, mut rx) = mpsc::channel(4);
        let publisher = RouterTrafficPublisher::new("router-1".into(), tx);

        publisher.record("app.example.com".to_string());

        let event = rx.recv().await.expect("Expected traffic event");
        assert_eq!(event.hostname, "app.example.com");
        assert_eq!(event.router_id, "router-1");
        assert!(event.timestamp > 0);
    }

    #[tokio::test]
    async fn test_record_deduplicates_within_window() {
        let (tx, mut rx) = mpsc::channel(100);
        let publisher = RouterTrafficPublisher::new("router-1".into(), tx);

        publisher.record("busy.example.com".to_string());
        publisher.record("busy.example.com".to_string());

        let first = rx.recv().await.expect("expected first event");
        assert_eq!(first.hostname, "busy.example.com");

        let second = timeout(std::time::Duration::from_millis(100), rx.recv()).await;
        assert!(second.is_err(), "should deduplicate repeated events within window");
    }

    #[tokio::test]
    async fn test_record_deduplicates_per_hostname() {
        let (tx, mut rx) = mpsc::channel(100);
        let publisher = RouterTrafficPublisher::new("router-1".into(), tx);

        publisher.record("app1.example.com".to_string());
        publisher.record("app2.example.com".to_string());

        let first = rx.recv().await.expect("expected first event");
        let second = rx.recv().await.expect("expected second event");

        assert_ne!(first.hostname, second.hostname);
        assert!(
            (first.hostname == "app1.example.com" && second.hostname == "app2.example.com")
                || (first.hostname == "app2.example.com"
                    && second.hostname == "app1.example.com")
        );
    }
}
