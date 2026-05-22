use crate::hypervisor::{HypervisorType, VmHypervisor};
use crate::metrics::MetricsCollector;
use std::collections::HashMap;
use std::sync::Arc;

/// HTTP server exposing health and metrics endpoints.
pub struct AgentHttpServer {
    port: u16,
    metrics_collector: MetricsCollector,
    hypervisors: Arc<HashMap<HypervisorType, Arc<dyn VmHypervisor>>>,
}

impl AgentHttpServer {
    pub fn new(
        port: u16,
        metrics_collector: MetricsCollector,
        hypervisors: Arc<HashMap<HypervisorType, Arc<dyn VmHypervisor>>>,
    ) -> Self {
        Self {
            port,
            metrics_collector,
            hypervisors,
        }
    }

    /// Spawn the HTTP server as a background task.
    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        let app = axum::Router::new()
            .route("/health", axum::routing::get(Self::health_handler))
            .route("/metrics", axum::routing::get(Self::metrics_handler))
            .with_state((self.metrics_collector, self.hypervisors));

        let listener = match std::net::TcpListener::bind(("0.0.0.0", self.port)) {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(port = self.port, error = %e, "Failed to bind HTTP server");
                return tokio::spawn(async {});
            },
        };

        listener.set_nonblocking(true).ok();
        let listener = match tokio::net::TcpListener::from_std(listener) {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(port = self.port, error = %e, "Failed to convert to async listener");
                return tokio::spawn(async {});
            },
        };

        tracing::info!(port = self.port, "HTTP health/metrics server starting");

        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                tracing::error!(error = %e, "HTTP server exited");
            }
        })
    }

    async fn health_handler() -> &'static str {
        "ok"
    }

    async fn metrics_handler(
        axum::extract::State((_collector, hypervisors)): axum::extract::State<(
            MetricsCollector,
            Arc<HashMap<HypervisorType, Arc<dyn VmHypervisor>>>,
        )>,
    ) -> String {
        let mut output = String::new();

        // Basic agent up metric
        output.push_str("# TYPE mikrom_agent_up gauge\n");
        output.push_str("mikrom_agent_up 1\n");

        // Hypervisor count
        output.push_str("# TYPE mikrom_agent_hypervisors gauge\n");
        output.push_str(&format!("mikrom_agent_hypervisors {}\n", hypervisors.len()));

        // VM counts per hypervisor
        for (htype, hv) in hypervisors.iter() {
            let vms = hv.get_all_vms().await;
            let running = vms
                .iter()
                .filter(|v| v.status == crate::hypervisor::VmStatus::Running)
                .count();
            let label = format!("hypervisor=\"{:?}\"", htype);
            output.push_str("# TYPE mikrom_agent_vms_total gauge\n");
            output.push_str(&format!(
                "mikrom_agent_vms_total{{{label}}} {}\n",
                vms.len()
            ));
            output.push_str("# TYPE mikrom_agent_vms_running gauge\n");
            output.push_str(&format!(
                "mikrom_agent_vms_running{{{label}}} {}\n",
                running
            ));
        }

        output
    }
}
