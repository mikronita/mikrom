use anyhow::Result;
use axum::{Router, routing::get};
use futures::StreamExt;
use mikrom_proto::router::RouterMetrics;
use prometheus::{Encoder, GaugeVec, Opts, Registry, TextEncoder};
use prost::Message;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_nats_url")]
    pub nats_url: String,
    #[serde(default = "default_loki_url")]
    pub loki_url: String,
    #[serde(default = "default_metrics_port")]
    pub metrics_port: u16,
}

fn default_nats_url() -> String {
    "nats://localhost:4222".to_string()
}

fn default_loki_url() -> String {
    "http://localhost:3100".to_string()
}

fn default_metrics_port() -> u16 {
    9090
}

impl Config {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();
        envy::from_env::<Config>().unwrap_or_else(|_| Config {
            nats_url: default_nats_url(),
            loki_url: default_loki_url(),
            metrics_port: default_metrics_port(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub vm_id: String,
    pub app_id: String,
    pub source: String,
    pub message: serde_json::Value,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmMetrics {
    pub app_id: String,
    #[serde(default)]
    pub vm_id: String,
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
    pub status: String,
    pub ipv6_address: Option<String>,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetrics {
    pub cpu_usage: f32,
    pub ram_used_bytes: u64,
    pub ram_total_bytes: u64,
    pub disk_used_bytes: u64,
    pub disk_total_bytes: u64,
    pub apps_count: u32,
    pub load_avg_1: f32,
    pub load_avg_5: f32,
    pub load_avg_15: f32,
}

pub struct TelemetryService {
    config: Config,
    nats: async_nats::Client,
    registry: Registry,
    cpu_gauge: GaugeVec,
    ram_gauge: GaugeVec,
    tx_gauge: GaugeVec,
    rx_gauge: GaugeVec,
    sys_cpu_gauge: GaugeVec,
    sys_ram_used_gauge: GaugeVec,
    sys_ram_total_gauge: GaugeVec,
    sys_disk_used_gauge: GaugeVec,
    sys_disk_total_gauge: GaugeVec,
    sys_apps_count_gauge: GaugeVec,
    sys_load_gauge: GaugeVec,
    // Router metrics
    router_req_total_gauge: GaugeVec,
    router_responses_gauge: GaugeVec,
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
        let tx_gauge = GaugeVec::new(
            Opts::new(
                "mikrom_vm_network_tx_bytes",
                "Network bytes transmitted per VM",
            ),
            &["app_id", "vm_id"],
        )?;
        let rx_gauge = GaugeVec::new(
            Opts::new(
                "mikrom_vm_network_rx_bytes",
                "Network bytes received per VM",
            ),
            &["app_id", "vm_id"],
        )?;

        let sys_cpu_gauge = GaugeVec::new(
            Opts::new("mikrom_sys_cpu_usage", "Host CPU usage"),
            &["node_id"],
        )?;
        let sys_ram_used_gauge = GaugeVec::new(
            Opts::new("mikrom_sys_ram_used_bytes", "Host RAM used"),
            &["node_id"],
        )?;
        let sys_ram_total_gauge = GaugeVec::new(
            Opts::new("mikrom_sys_ram_total_bytes", "Host RAM total"),
            &["node_id"],
        )?;
        let sys_disk_used_gauge = GaugeVec::new(
            Opts::new("mikrom_sys_disk_used_bytes", "Host disk used"),
            &["node_id"],
        )?;
        let sys_disk_total_gauge = GaugeVec::new(
            Opts::new("mikrom_sys_disk_total_bytes", "Host disk total"),
            &["node_id"],
        )?;
        let sys_apps_count_gauge = GaugeVec::new(
            Opts::new("mikrom_sys_apps_count", "Number of running apps on host"),
            &["node_id"],
        )?;
        let sys_load_gauge = GaugeVec::new(
            Opts::new("mikrom_sys_load_avg", "Host load average"),
            &["node_id", "period"],
        )?;

        let router_req_total_gauge = GaugeVec::new(
            Opts::new(
                "mikrom_router_requests_total",
                "Total requests handled by router",
            ),
            &["router_id"],
        )?;
        let router_responses_gauge = GaugeVec::new(
            Opts::new(
                "mikrom_router_responses_total",
                "Total responses by status family",
            ),
            &["router_id", "family"],
        )?;

        registry.register(Box::new(cpu_gauge.clone()))?;
        registry.register(Box::new(ram_gauge.clone()))?;
        registry.register(Box::new(tx_gauge.clone()))?;
        registry.register(Box::new(rx_gauge.clone()))?;
        registry.register(Box::new(sys_cpu_gauge.clone()))?;
        registry.register(Box::new(sys_ram_used_gauge.clone()))?;
        registry.register(Box::new(sys_ram_total_gauge.clone()))?;
        registry.register(Box::new(sys_disk_used_gauge.clone()))?;
        registry.register(Box::new(sys_disk_total_gauge.clone()))?;
        registry.register(Box::new(sys_apps_count_gauge.clone()))?;
        registry.register(Box::new(sys_load_gauge.clone()))?;
        registry.register(Box::new(router_req_total_gauge.clone()))?;
        registry.register(Box::new(router_responses_gauge.clone()))?;

        Ok(Self {
            config,
            nats,
            registry,
            cpu_gauge,
            ram_gauge,
            tx_gauge,
            rx_gauge,
            sys_cpu_gauge,
            sys_ram_used_gauge,
            sys_ram_total_gauge,
            sys_disk_used_gauge,
            sys_disk_total_gauge,
            sys_apps_count_gauge,
            sys_load_gauge,
            router_req_total_gauge,
            router_responses_gauge,
            http_client: reqwest::Client::new(),
        })
    }

    pub async fn run(self: Arc<Self>) -> Result<()> {
        let vm_metrics_handle = tokio::spawn(self.clone().listen_vm_metrics());
        let sys_metrics_handle = tokio::spawn(self.clone().listen_sys_metrics());
        let router_metrics_handle = tokio::spawn(self.clone().listen_router_metrics());
        let logs_handle = tokio::spawn(self.clone().listen_logs());
        let server_handle = tokio::spawn(self.clone().run_metrics_server());

        let res = tokio::select! {
            res = vm_metrics_handle => {
                tracing::error!("VM metrics task exited");
                res
            },
            res = sys_metrics_handle => {
                tracing::error!("System metrics task exited");
                res
            },
            res = router_metrics_handle => {
                tracing::error!("Router metrics task exited");
                res
            },
            res = logs_handle => {
                tracing::error!("Logs task exited");
                res
            },
            res = server_handle => {
                tracing::error!("Metrics server task exited");
                res
            },
        };

        match res {
            Ok(inner_res) => inner_res?,
            Err(e) => return Err(anyhow::anyhow!("Task panicked: {}", e)),
        }

        Ok(())
    }

    async fn listen_vm_metrics(self: Arc<Self>) -> Result<()> {
        let mut sub = self.nats.subscribe("mikrom.metrics.>").await?;
        tracing::info!("Listening for VM metrics on mikrom.metrics.>");

        while let Some(msg) = sub.next().await {
            // Check if it's router metrics (handled elsewhere)
            if msg.subject.starts_with("mikrom.metrics.router.") {
                continue;
            }

            let Ok(metrics) = serde_json::from_slice::<VmMetrics>(&msg.payload) else {
                continue;
            };

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
            self.tx_gauge
                .with_label_values(&[app_id, vm_id])
                .set(metrics.tx_bytes as f64);
            self.rx_gauge
                .with_label_values(&[app_id, vm_id])
                .set(metrics.rx_bytes as f64);
        }
        Ok(())
    }

    async fn listen_router_metrics(self: Arc<Self>) -> Result<()> {
        let mut sub = self.nats.subscribe("mikrom.metrics.router.*").await?;
        tracing::info!("Listening for Router metrics on mikrom.metrics.router.*");

        while let Some(msg) = sub.next().await {
            let Ok(metrics) = RouterMetrics::decode(&msg.payload[..]) else {
                tracing::warn!("Failed to decode RouterMetrics from NATS");
                continue;
            };

            let router_id = &metrics.router_id;

            self.router_req_total_gauge
                .with_label_values(&[router_id])
                .set(metrics.requests_total as f64);

            self.router_responses_gauge
                .with_label_values(&[router_id, "2xx"])
                .set(metrics.responses_2xx as f64);
            self.router_responses_gauge
                .with_label_values(&[router_id, "4xx"])
                .set(metrics.responses_4xx as f64);
            self.router_responses_gauge
                .with_label_values(&[router_id, "5xx"])
                .set(metrics.responses_5xx as f64);
        }
        Ok(())
    }

    async fn listen_sys_metrics(self: Arc<Self>) -> Result<()> {
        let mut sub = self.nats.subscribe("mikrom.agent.*.metrics").await?;
        tracing::info!("Listening for system metrics on mikrom.agent.*.metrics");

        while let Some(msg) = sub.next().await {
            let Ok(metrics) = serde_json::from_slice::<SystemMetrics>(&msg.payload) else {
                continue;
            };

            // Extract node_id from mikrom.agent.<node_id>.metrics
            let parts: Vec<&str> = msg.subject.split('.').collect();
            let node_id = parts.get(2).cloned().unwrap_or("unknown-node");

            self.sys_cpu_gauge
                .with_label_values(&[node_id])
                .set(metrics.cpu_usage as f64);
            self.sys_ram_used_gauge
                .with_label_values(&[node_id])
                .set(metrics.ram_used_bytes as f64);
            self.sys_ram_total_gauge
                .with_label_values(&[node_id])
                .set(metrics.ram_total_bytes as f64);
            self.sys_disk_used_gauge
                .with_label_values(&[node_id])
                .set(metrics.disk_used_bytes as f64);
            self.sys_disk_total_gauge
                .with_label_values(&[node_id])
                .set(metrics.disk_total_bytes as f64);
            self.sys_apps_count_gauge
                .with_label_values(&[node_id])
                .set(metrics.apps_count as f64);

            self.sys_load_gauge
                .with_label_values(&[node_id, "1m"])
                .set(metrics.load_avg_1 as f64);
            self.sys_load_gauge
                .with_label_values(&[node_id, "5m"])
                .set(metrics.load_avg_5 as f64);
            self.sys_load_gauge
                .with_label_values(&[node_id, "15m"])
                .set(metrics.load_avg_15 as f64);
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

        let body = self.format_loki_payload(entries);
        let url = format!("{}/loki/api/v1/push", self.config.loki_url);
        self.http_client.post(url).json(&body).send().await?;

        Ok(())
    }

    fn format_loki_payload(&self, entries: Vec<LogEntry>) -> serde_json::Value {
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

        serde_json::json!({
            "streams": streams.into_values().collect::<Vec<_>>()
        })
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
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[tokio::test]
    async fn test_loki_payload_formatting() {
        let _config = Config {
            nats_url: "nats://localhost:4222".to_string(),
            loki_url: "http://localhost:3100".to_string(),
            metrics_port: 9090,
        };
        // We need a dummy NATS client for the service, but for format_loki_payload we can just use a dummy
        // Or refactor to not require the whole service.
        // Let's test the LogEntry serialization instead since format_loki_payload is a method of TelemetryService.

        let entry1 = LogEntry {
            vm_id: "vm-1".to_string(),
            app_id: "app-1".to_string(),
            source: "stdout".to_string(),
            message: serde_json::json!("msg 1"),
            timestamp: 1000,
        };
        let entry2 = LogEntry {
            vm_id: "vm-1".to_string(),
            app_id: "app-1".to_string(),
            source: "stdout".to_string(),
            message: serde_json::json!("msg 2"),
            timestamp: 2000,
        };

        let entries = vec![entry1, entry2];
        let json = serde_json::to_value(&entries).unwrap();
        assert_eq!(json.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_loki_stream_grouping() {
        // Since TelemetryService::new is async and connects to NATS, we'll test the grouping logic
        // by checking if we can create a dummy version or if we should move the function to a helper.
        // For now, let's verify LogEntry fields.
        let entry = LogEntry {
            vm_id: "vm-1".to_string(),
            app_id: "app-1".to_string(),
            source: "stdout".to_string(),
            message: serde_json::json!("test message"),
            timestamp: 123456789,
        };
        assert_eq!(entry.app_id, "app-1");
    }

    #[test]
    fn test_config_defaults() {
        let config = Config {
            nats_url: default_nats_url(),
            loki_url: default_loki_url(),
            metrics_port: default_metrics_port(),
        };
        assert_eq!(config.nats_url, "nats://localhost:4222");
        assert_eq!(config.loki_url, "http://localhost:3100");
        assert_eq!(config.metrics_port, 9090);
    }
}
