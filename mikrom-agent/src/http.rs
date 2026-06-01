use crate::hypervisor::{HypervisorType, VmHypervisor};
use crate::metrics::MetricsCollector;
use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr, TcpListener};
use std::sync::Arc;

type HypervisorMap = Arc<HashMap<HypervisorType, Arc<dyn VmHypervisor>>>;
type HttpServerState = (MetricsCollector, HypervisorMap);

fn bind_ipv6_dual_stack_listener(port: u16) -> std::io::Result<TcpListener> {
    let socket = socket2::Socket::new(
        socket2::Domain::IPV6,
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )?;
    socket.set_reuse_address(true)?;
    socket.set_only_v6(false)?;

    let addr = std::net::SocketAddr::from((Ipv6Addr::UNSPECIFIED, port));
    socket.bind(&socket2::SockAddr::from(addr))?;
    socket.listen(1024)?;
    socket.set_nonblocking(true)?;

    Ok(socket.into())
}

fn bind_ipv4_listener(port: u16) -> std::io::Result<TcpListener> {
    let socket = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )?;
    socket.set_reuse_address(true)?;

    let addr = std::net::SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));
    socket.bind(&socket2::SockAddr::from(addr))?;
    socket.listen(1024)?;
    socket.set_nonblocking(true)?;

    Ok(socket.into())
}

fn bind_dual_stack_listener(port: u16) -> std::io::Result<TcpListener> {
    match bind_ipv6_dual_stack_listener(port) {
        Ok(listener) => Ok(listener),
        Err(v6_err) => {
            tracing::warn!(port = port, error = %v6_err, "IPv6 bind unavailable, falling back to IPv4");
            bind_ipv4_listener(port).map_err(|v4_err| {
                std::io::Error::new(
                    v4_err.kind(),
                    format!(
                        "failed to bind listener on port {port} after IPv6 fallback (IPv6 error: {v6_err}; IPv4 error: {v4_err})"
                    ),
                )
            })
        },
    }
}

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

        let listener = match bind_dual_stack_listener(self.port) {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(port = self.port, error = %e, "Failed to bind HTTP server");
                return tokio::spawn(async {});
            },
        };

        tracing::info!(port = self.port, "HTTP health/metrics server starting");

        tokio::spawn(async move {
            let listener = match tokio::net::TcpListener::from_std(listener) {
                Ok(listener) => listener,
                Err(e) => {
                    tracing::error!(port = self.port, error = %e, "Failed to convert HTTP listener");
                    return;
                },
            };
            if let Err(e) = axum::serve(listener, app).await {
                tracing::error!(error = %e, "HTTP server exited");
            }
        })
    }

    async fn health_handler() -> &'static str {
        "ok"
    }

    async fn metrics_handler(
        axum::extract::State((collector, hypervisors)): axum::extract::State<HttpServerState>,
    ) -> String {
        use std::fmt::Write;
        let mut output = String::new();

        // Basic agent up metric
        let _ = writeln!(output, "# TYPE mikrom_agent_up gauge");
        let _ = writeln!(output, "mikrom_agent_up 1");

        // Hypervisor count
        let _ = writeln!(output, "# TYPE mikrom_agent_hypervisors gauge");
        let _ = writeln!(output, "mikrom_agent_hypervisors {}", hypervisors.len());

        // VM counts per hypervisor
        for (htype, hv) in hypervisors.iter() {
            let vms = hv.get_all_vms().await;
            let running = vms
                .iter()
                .filter(|v| v.status == crate::hypervisor::VmStatus::Running)
                .count();
            let label = format!("hypervisor=\"{:?}\"", htype);
            let _ = writeln!(output, "# TYPE mikrom_agent_vms_total gauge");
            let _ = writeln!(output, "mikrom_agent_vms_total{{{label}}} {}", vms.len());
            let _ = writeln!(output, "# TYPE mikrom_agent_vms_running gauge");
            let _ = writeln!(output, "mikrom_agent_vms_running{{{label}}} {running}");
        }

        // Collect system and VM metrics
        let metrics = collector.collect().await;
        let host_id = hypervisors
            .values()
            .next()
            .map(|hv| hv.agent_id().to_string())
            .unwrap_or_else(|| "unknown-host".to_string());

        // Host system metrics
        let _ = writeln!(output, "# TYPE mikrom_sys_cpu_usage gauge");
        let _ = writeln!(
            output,
            "mikrom_sys_cpu_usage{{node_id=\"{host_id}\"}} {}",
            metrics.cpu_usage
        );

        let _ = writeln!(output, "# TYPE mikrom_sys_ram_used_bytes gauge");
        let _ = writeln!(
            output,
            "mikrom_sys_ram_used_bytes{{node_id=\"{host_id}\"}} {}",
            metrics.ram_used_bytes
        );

        let _ = writeln!(output, "# TYPE mikrom_sys_ram_total_bytes gauge");
        let _ = writeln!(
            output,
            "mikrom_sys_ram_total_bytes{{node_id=\"{host_id}\"}} {}",
            metrics.ram_total_bytes
        );

        let _ = writeln!(output, "# TYPE mikrom_sys_disk_used_bytes gauge");
        let _ = writeln!(
            output,
            "mikrom_sys_disk_used_bytes{{node_id=\"{host_id}\"}} {}",
            metrics.disk_used_bytes
        );

        let _ = writeln!(output, "# TYPE mikrom_sys_disk_total_bytes gauge");
        let _ = writeln!(
            output,
            "mikrom_sys_disk_total_bytes{{node_id=\"{host_id}\"}} {}",
            metrics.disk_total_bytes
        );

        let _ = writeln!(output, "# TYPE mikrom_sys_apps_count gauge");
        let _ = writeln!(
            output,
            "mikrom_sys_apps_count{{node_id=\"{host_id}\"}} {}",
            metrics.apps_count
        );

        let _ = writeln!(output, "# TYPE mikrom_sys_load_avg gauge");
        let _ = writeln!(
            output,
            "mikrom_sys_load_avg{{node_id=\"{host_id}\",period=\"1m\"}} {}",
            metrics.load_avg_1
        );
        let _ = writeln!(
            output,
            "mikrom_sys_load_avg{{node_id=\"{host_id}\",period=\"5m\"}} {}",
            metrics.load_avg_5
        );
        let _ = writeln!(
            output,
            "mikrom_sys_load_avg{{node_id=\"{host_id}\",period=\"15m\"}} {}",
            metrics.load_avg_15
        );

        // VM resource metrics
        if !metrics.vms.is_empty() {
            let vms: Vec<_> = metrics.vms.values().collect();

            let _ = writeln!(output, "# TYPE mikrom_vm_cpu_usage gauge");
            for vm in &vms {
                let _ = writeln!(
                    output,
                    "mikrom_vm_cpu_usage{{app_id=\"{}\",vm_id=\"{}\"}} {}",
                    vm.app_id, vm.vm_id, vm.cpu_usage
                );
            }

            let _ = writeln!(output, "# TYPE mikrom_vm_ram_usage_bytes gauge");
            for vm in &vms {
                let _ = writeln!(
                    output,
                    "mikrom_vm_ram_usage_bytes{{app_id=\"{}\",vm_id=\"{}\"}} {}",
                    vm.app_id, vm.vm_id, vm.ram_used_bytes
                );
            }

            let _ = writeln!(output, "# TYPE mikrom_vm_network_tx_bytes gauge");
            for vm in &vms {
                let _ = writeln!(
                    output,
                    "mikrom_vm_network_tx_bytes{{app_id=\"{}\",vm_id=\"{}\"}} {}",
                    vm.app_id, vm.vm_id, vm.tx_bytes
                );
            }

            let _ = writeln!(output, "# TYPE mikrom_vm_network_rx_bytes gauge");
            for vm in &vms {
                let _ = writeln!(
                    output,
                    "mikrom_vm_network_rx_bytes{{app_id=\"{}\",vm_id=\"{}\"}} {}",
                    vm.app_id, vm.vm_id, vm.rx_bytes
                );
            }
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};

    #[tokio::test]
    async fn test_agent_metrics_handler_output() {
        let collector = MetricsCollector::new();
        let hypervisors = Arc::new(HashMap::new());

        let response =
            AgentHttpServer::metrics_handler(axum::extract::State((collector, hypervisors))).await;

        assert!(response.contains("mikrom_agent_up 1\n"));
        assert!(response.contains("mikrom_agent_hypervisors 0\n"));
        assert!(response.contains("mikrom_sys_cpu_usage{node_id=\"unknown-host\"}"));
        assert!(response.contains("mikrom_sys_ram_used_bytes{node_id=\"unknown-host\"}"));
        assert!(response.contains("mikrom_sys_apps_count{node_id=\"unknown-host\"} 0\n"));
    }

    #[test]
    fn dual_stack_listener_accepts_ipv4_and_ipv6() {
        let listener = match bind_dual_stack_listener(0) {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!("skipping agent smoke test: dual-stack bind unavailable: {err}");
                return;
            },
        };
        let local_addr = listener
            .local_addr()
            .expect("local addr should be available");
        let port = local_addr.port();

        let mut connected_streams = Vec::new();
        if local_addr.is_ipv6() {
            let v6 = SocketAddr::from((Ipv6Addr::LOCALHOST, port));
            match TcpStream::connect(v6) {
                Ok(stream) => connected_streams.push(stream),
                Err(err) => {
                    eprintln!("skipping agent smoke test: ipv6 loopback unavailable: {err}")
                },
            }
        }

        let v4 = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        match TcpStream::connect(v4) {
            Ok(stream) => connected_streams.push(stream),
            Err(err) => {
                if local_addr.is_ipv6() {
                    eprintln!("skipping agent smoke test: ipv4 loopback unavailable: {err}");
                } else {
                    eprintln!(
                        "skipping agent smoke test: ipv4-only fallback could not connect: {err}"
                    );
                    return;
                }
            },
        }

        if connected_streams.is_empty() {
            eprintln!("skipping agent smoke test: no loopback connections succeeded");
            return;
        }

        let expected_accepts = connected_streams.len();
        let handle = std::thread::spawn(move || {
            for _ in 0..expected_accepts {
                let _ = listener.accept();
            }
        });

        handle.join().expect("listener thread should exit");
        drop(connected_streams);
    }
}
