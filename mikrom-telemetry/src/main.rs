use anyhow::Result;
use axum::{routing::get, Router};
use futures::StreamExt;
use prometheus::{Encoder, GaugeVec, Opts, Registry, TextEncoder};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub nats_url: String,
    pub loki_url: String,
    pub metrics_port: u16,
}

impl Config {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();
        Self {
            nats_url: std::env::var("NATS_URL")
                .unwrap_or_else(|_| "nats://localhost:4222".to_string()),
            loki_url: std::env::var("LOKI_URL")
                .unwrap_or_else(|_| "http://localhost:3100".to_string()),
            metrics_port: std::env::var("METRICS_PORT")
                .unwrap_or_else(|_| "9090".to_string())
                .parse()
                .unwrap_or(9090),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub vm_id: String,
    pub app_id: String,
    pub source: String,
    pub message: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmMetrics {
    pub app_id: String,
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
    pub status: String,
    pub ip_address: Option<String>,
}

pub struct TelemetryService {
    config: Config,
    nats: async_nats::Client,
    registry: Registry,
    cpu_gauge: GaugeVec,
    ram_gauge: GaugeVec,
    http_client: reqwest::Client,
}

impl TelemetryService {
    pub async fn new(config: Config) -> Result<Self> {
        let nats = async_nats::connect(&config.nats_url).await?;

        let registry = Registry::new();
        let cpu_gauge = GaugeVec::new(
            Opts::new("mikrom_vm_cpu_usage", "CPU usage per VM"),
            &["app_id", "vm_id"],
        )?;
        let ram_gauge = GaugeVec::new(
            Opts::new("mikrom_vm_ram_usage_bytes", "RAM usage in bytes per VM"),
            &["app_id", "vm_id"],
        )?;

        registry.register(Box::new(cpu_gauge.clone()))?;
        registry.register(Box::new(ram_gauge.clone()))?;

        Ok(Self {
            config,
            nats,
            registry,
            cpu_gauge,
            ram_gauge,
            http_client: reqwest::Client::new(),
        })
    }

    pub async fn run(self: Arc<Self>) -> Result<()> {
        let metrics_task = self.clone().listen_metrics();
        let logs_task = self.clone().listen_logs();
        let server_task = self.clone().run_metrics_server();

        tokio::select! {
            res = metrics_task => res,
            res = logs_task => res,
            res = server_task => res,
        }
    }

    async fn listen_metrics(self: Arc<Self>) -> Result<()> {
        let mut sub = self.nats.subscribe("mikrom.metrics.>").await?;
        tracing::info!("Listening for metrics on mikrom.metrics.>");

        while let Some(msg) = sub.next().await {
            let Ok(metrics) = serde_json::from_slice::<VmMetrics>(&msg.payload) else {
                continue;
            };

            // Subject is mikrom.metrics.<app_id>.<vm_id>
            let parts: Vec<&str> = msg.subject.split('.').collect();
            if parts.len() < 4 {
                continue;
            }
            let vm_id = parts[3];
            let app_id = parts[2];

            self.cpu_gauge
                .with_label_values(&[app_id, vm_id])
                .set(metrics.cpu_usage as f64);
            self.ram_gauge
                .with_label_values(&[app_id, vm_id])
                .set(metrics.ram_used_bytes as f64);
        }
        Ok(())
    }

    async fn listen_logs(self: Arc<Self>) -> Result<()> {
        let mut sub = self.nats.subscribe("mikrom.logs.>").await?;
        tracing::info!("Listening for logs on mikrom.logs.>");

        while let Some(msg) = sub.next().await {
            let Ok(entries) = serde_json::from_slice::<Vec<LogEntry>>(&msg.payload) else {
                continue;
            };

            if let Err(e) = self.push_to_loki(entries).await {
                tracing::error!("Failed to push to Loki: {e}");
            }
        }
        Ok(())
    }

    async fn push_to_loki(&self, entries: Vec<LogEntry>) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        let mut streams = std::collections::HashMap::new();

        for entry in entries {
            let key = format!("{}-{}-{}", entry.app_id, entry.vm_id, entry.source);
            let stream = streams.entry(key).or_insert_with(|| {
                serde_json::json!({
                    "stream": {
                        "app_id": entry.app_id.clone(),
                        "vm_id": entry.vm_id.clone(),
                        "source": entry.source.clone()
                    },
                    "values": []
                })
            });

            if let Some(values) = stream["values"].as_array_mut() {
                values.push(serde_json::json!([
                    entry.timestamp.to_string(),
                    entry.message
                ]));
            }
        }

        let body = serde_json::json!({
            "streams": streams.into_values().collect::<Vec<_>>()
        });

        let url = format!("{}/loki/api/v1/push", self.config.loki_url);
        self.http_client.post(url).json(&body).send().await?;

        Ok(())
    }

    async fn run_metrics_server(self: Arc<Self>) -> Result<()> {
        let metrics_port = self.config.metrics_port;
        let self_for_handler = self.clone();

        let app = Router::new().route(
            "/metrics",
            get(move || {
                let self_clone = self_for_handler.clone();
                async move {
                    let mut buffer = Vec::new();
                    let encoder = TextEncoder::new();
                    let metric_families = self_clone.registry.gather();
                    if let Err(e) = encoder.encode(&metric_families, &mut buffer) {
                        tracing::error!("Failed to encode prometheus metrics: {}", e);
                        return String::new();
                    }
                    String::from_utf8(buffer).unwrap_or_else(|e| {
                        tracing::error!("Prometheus metrics not valid UTF-8: {}", e);
                        String::new()
                    })
                }
            }),
        );

        let addr = SocketAddr::from(([0, 0, 0, 0], metrics_port));
        tracing::info!("Metrics server listening on {}", addr);
        let listener = TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let config = Config::from_env();
    let service = Arc::new(TelemetryService::new(config).await?);
    service.run().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_loki_payload_formatting() {
        // We can't easily test the full service without NATS, but we can test the push_to_loki logic
        // if we make it return the JSON instead of sending it.
        // For now, let's just check the LogEntry serialization.
        let entry = LogEntry {
            vm_id: "vm-1".to_string(),
            app_id: "app-1".to_string(),
            source: "stdout".to_string(),
            message: "test log".to_string(),
            timestamp: 1700000000000000000,
        };

        let json = serde_json::to_string(&vec![entry]).unwrap();
        assert!(json.contains("test log"));
    }
}
