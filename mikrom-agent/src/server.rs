use crate::firecracker::{FirecrackerManager, VmConfig};
use crate::metrics::MetricsCollector;
use mikrom_proto::agent::{
    GetLogsRequest, GetLogsResponse, GetMetricsRequest, GetMetricsResponse, GetVmStatusRequest,
    GetVmStatusResponse, MetricsRequest, MetricsResponse, PauseVmRequest, PauseVmResponse,
    RegisterRequest, RegisterResponse, ResumeVmRequest, ResumeVmResponse, StartVmRequest,
    StartVmResponse, StopVmRequest, StopVmResponse, UnregisterRequest, UnregisterResponse,
    agent_service_server::{AgentService, AgentServiceServer},
};

use mikrom_proto::scheduler::{
    RegisterWorkerRequest, ReportMetricsRequest, SchedulerServiceClient,
};
use mikrom_proto::tls::ServiceCerts;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tonic::{Response, Status, async_trait};
use uuid::Uuid;

pub struct AgentServer {
    host_id: String,
    hostname: String,
    ip_address: String,
    metrics_collector: MetricsCollector,
    firecracker: FirecrackerManager,
    scheduler_client: Option<SchedulerClient>,
    shutdown_flag: Arc<RwLock<bool>>,
    scheduler_addr: String,
}

#[derive(Clone)]
#[allow(dead_code)]
struct SchedulerClient {
    host_id: String,
    channel: tonic::transport::Channel,
}

#[async_trait]
impl AgentService for AgentServer {
    async fn register(
        &self,
        request: tonic::Request<RegisterRequest>,
    ) -> Result<Response<RegisterResponse>, Status> {
        let req = request.into_inner();
        tracing::info!("Registering agent: {}", req.host_id);
        Ok(Response::new(RegisterResponse {
            success: true,
            message: "Registered successfully".to_string(),
        }))
    }

    async fn unregister(
        &self,
        request: tonic::Request<UnregisterRequest>,
    ) -> Result<Response<UnregisterResponse>, Status> {
        let req = request.into_inner();
        tracing::info!("Unregistering agent: {}", req.host_id);
        *self.shutdown_flag.write() = true;
        Ok(Response::new(UnregisterResponse {
            success: true,
            message: "Unregistered successfully".to_string(),
        }))
    }

    async fn report_metrics(
        &self,
        request: tonic::Request<MetricsRequest>,
    ) -> Result<Response<MetricsResponse>, Status> {
        let req = request.into_inner();
        tracing::debug!(
            "Reported metrics: cpu={:.2}, ram={}/{} load={:.2}/{:.2}/{:.2}",
            req.cpu_usage,
            req.ram_used_bytes,
            req.ram_total_bytes,
            req.load_avg_1,
            req.load_avg_5,
            req.load_avg_15
        );
        Ok(Response::new(MetricsResponse { success: true }))
    }

    async fn get_metrics(
        &self,
        _request: tonic::Request<GetMetricsRequest>,
    ) -> Result<Response<GetMetricsResponse>, Status> {
        let metrics = self.metrics_collector.collect().await;
        Ok(Response::new(GetMetricsResponse {
            host_id: self.host_id.clone(),
            cpu_usage: metrics.cpu_usage,
            ram_used_bytes: metrics.ram_used_bytes,
            ram_total_bytes: metrics.ram_total_bytes,
            disk_used_bytes: metrics.disk_used_bytes,
            disk_total_bytes: metrics.disk_total_bytes,
            apps_count: metrics.apps_count,
            timestamp: metrics.timestamp,
            load_avg_1: metrics.load_avg_1,
            load_avg_5: metrics.load_avg_5,
            load_avg_15: metrics.load_avg_15,
            vms: metrics
                .vms
                .into_iter()
                .map(|(id, m)| {
                    (
                        id,
                        mikrom_proto::agent::VmMetrics {
                            cpu_usage: m.cpu_usage,
                            ram_used_bytes: m.ram_used_bytes,
                        },
                    )
                })
                .collect(),
        }))
    }

    async fn get_logs(
        &self,
        request: tonic::Request<GetLogsRequest>,
    ) -> Result<Response<Self::GetLogsStream>, Status> {
        let req = request.into_inner();
        let vm_id = req.vm_id;
        let follow = req.follow;
        tracing::info!(
            "Log streaming requested for VM: {} (follow={})",
            vm_id,
            follow
        );
        let firecracker = self.firecracker.clone();

        let (tx, rx) = tokio::sync::mpsc::channel(100);

        tokio::spawn(async move {
            // Send existing logs first
            let initial_logs = firecracker.get_logs(&vm_id).await;
            let count = initial_logs.len();
            for line in &initial_logs {
                if tx
                    .send(Ok(GetLogsResponse {
                        line: line.clone(),
                        timestamp: chrono::Utc::now().timestamp(),
                    }))
                    .await
                    .is_err()
                {
                    return;
                }
            }

            if follow {
                let mut last_index = count;
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    let current_logs = firecracker.get_logs(&vm_id).await;
                    if current_logs.len() > last_index {
                        for line in &current_logs[last_index..] {
                            if tx
                                .send(Ok(GetLogsResponse {
                                    line: line.clone(),
                                    timestamp: chrono::Utc::now().timestamp(),
                                }))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                        last_index = current_logs.len();
                    }
                }
            }
        });

        let output_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(output_stream) as Self::GetLogsStream))
    }

    type GetLogsStream =
        std::pin::Pin<Box<dyn tokio_stream::Stream<Item = Result<GetLogsResponse, Status>> + Send>>;

    async fn start_vm(
        &self,
        request: tonic::Request<StartVmRequest>,
    ) -> Result<Response<StartVmResponse>, Status> {
        let req = request.into_inner();
        tracing::info!(
            vm_id = %req.vm_id,
            app_id = %req.app_id,
            image = %req.image,
            "Handling start_vm request"
        );

        let vm_id = if req.vm_id.is_empty() {
            Uuid::new_v4().to_string()
        } else {
            req.vm_id.clone()
        };

        let mut env = HashMap::new();
        if let Some(config) = &req.config {
            for (k, v) in &config.env {
                env.insert(k.clone(), v.clone());
            }
        }

        let config = VmConfig {
            vcpus: req.config.as_ref().map(|c| c.vcpus).unwrap_or(1),
            memory_mib: req
                .config
                .as_ref()
                .map(|c| c.memory_mib as u64)
                .unwrap_or(256),
            disk_mib: req
                .config
                .as_ref()
                .map(|c| c.disk_mib as u64)
                .unwrap_or(1024),
            env,
            ip_address: req.config.as_ref().and_then(|c| {
                if c.ip_address.is_empty() {
                    None
                } else {
                    Some(c.ip_address.clone())
                }
            }),
            gateway: req.config.as_ref().and_then(|c| {
                if c.gateway.is_empty() {
                    None
                } else {
                    Some(c.gateway.clone())
                }
            }),
            mac_address: req.config.as_ref().and_then(|c| {
                if c.mac_address.is_empty() {
                    None
                } else {
                    Some(c.mac_address.clone())
                }
            }),
            volumes: req
                .config
                .as_ref()
                .map(|c| {
                    c.volumes
                        .iter()
                        .map(|v| crate::firecracker::Volume {
                            volume_id: v.volume_id.clone(),
                            size_mib: v.size_mib,
                            read_only: v.read_only,
                        })
                        .collect()
                })
                .unwrap_or_default(),
        };

        tracing::info!("Starting VM {} with config: {:?}", vm_id, config);

        match self
            .firecracker
            .start_vm(vm_id.clone(), req.app_id, req.image, config)
            .await
        {
            Ok(()) => {
                self.metrics_collector.increment_app_count();
                Ok(Response::new(StartVmResponse {
                    success: true,
                    vm_id,
                    message: "VM started".to_string(),
                }))
            }
            Err(e) => Ok(Response::new(StartVmResponse {
                success: false,
                vm_id: String::new(),
                message: e.to_string(),
            })),
        }
    }

    async fn stop_vm(
        &self,
        request: tonic::Request<StopVmRequest>,
    ) -> Result<Response<StopVmResponse>, Status> {
        let req = request.into_inner();
        tracing::info!(vm_id = %req.vm_id, "Handling stop_vm request");

        match self.firecracker.stop_vm(&req.vm_id).await {
            Ok(()) => {
                self.metrics_collector.decrement_app_count();
                Ok(Response::new(StopVmResponse {
                    success: true,
                    message: "VM stopped".to_string(),
                }))
            }
            Err(e) => Ok(Response::new(StopVmResponse {
                success: false,
                message: e.to_string(),
            })),
        }
    }

    async fn pause_vm(
        &self,
        request: tonic::Request<PauseVmRequest>,
    ) -> Result<Response<PauseVmResponse>, Status> {
        let req = request.into_inner();
        tracing::info!(vm_id = %req.vm_id, "Handling pause_vm request");

        match self.firecracker.pause_vm(&req.vm_id).await {
            Ok(()) => Ok(Response::new(PauseVmResponse {
                success: true,
                message: "VM paused".to_string(),
            })),
            Err(e) => Ok(Response::new(PauseVmResponse {
                success: false,
                message: e.to_string(),
            })),
        }
    }

    async fn resume_vm(
        &self,
        request: tonic::Request<ResumeVmRequest>,
    ) -> Result<Response<ResumeVmResponse>, Status> {
        let req = request.into_inner();
        tracing::info!(vm_id = %req.vm_id, "Handling resume_vm request");

        match self.firecracker.resume_vm(&req.vm_id).await {
            Ok(()) => Ok(Response::new(ResumeVmResponse {
                success: true,
                message: "VM resumed".to_string(),
            })),
            Err(e) => Ok(Response::new(ResumeVmResponse {
                success: false,
                message: e.to_string(),
            })),
        }
    }

    async fn get_vm_status(
        &self,
        request: tonic::Request<GetVmStatusRequest>,
    ) -> Result<Response<GetVmStatusResponse>, Status> {
        let req = request.into_inner();

        match self.firecracker.get_vm_status(&req.vm_id).await {
            Ok(status) => {
                let proto_status = match status {
                    crate::firecracker::VmStatus::Starting => 1,
                    crate::firecracker::VmStatus::Running => 2,
                    crate::firecracker::VmStatus::Stopping => 3,
                    crate::firecracker::VmStatus::Stopped => 4,
                    crate::firecracker::VmStatus::Failed => 5,
                    crate::firecracker::VmStatus::Paused => 6,
                };
                Ok(Response::new(GetVmStatusResponse {
                    vm_id: req.vm_id,
                    status: proto_status,
                    started_at: 0,
                    error_message: String::new(),
                }))
            }
            Err(e) => Err(Status::not_found(e.to_string())),
        }
    }
}

impl AgentServer {
    pub fn new(host_id: String, hostname: String, ip_address: String) -> Self {
        let scheduler_addr =
            std::env::var("SCHEDULER_ADDR").unwrap_or_else(|_| "http://127.0.0.1:5002".to_string());
        Self::with_scheduler_addr(host_id, hostname, ip_address, scheduler_addr)
    }

    /// Create an agent that connects to the given scheduler address.
    /// Useful for integration tests where the scheduler runs on a random port.
    pub fn with_scheduler_addr(
        host_id: String,
        hostname: String,
        ip_address: String,
        scheduler_addr: String,
    ) -> Self {
        let firecracker = FirecrackerManager::new();
        Self::with_manager(host_id, hostname, ip_address, scheduler_addr, firecracker)
    }

    pub fn with_manager(
        host_id: String,
        hostname: String,
        ip_address: String,
        scheduler_addr: String,
        firecracker: FirecrackerManager,
    ) -> Self {
        Self {
            host_id,
            hostname,
            ip_address,
            metrics_collector: MetricsCollector::with_firecracker(firecracker.clone()),
            firecracker,
            scheduler_client: None,
            shutdown_flag: Arc::new(RwLock::new(false)),
            scheduler_addr,
        }
    }

    pub async fn serve(
        &self,
        addr: SocketAddr,
        use_tls: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let host_id = self.host_id.clone();
        let hostname = self.hostname.clone();
        let ip_address = self.ip_address.clone();
        let metrics_collector = self.metrics_collector.clone();
        let agent_port = addr.port();

        // Load certs once — they are moved into the background task and also
        // used to configure the gRPC server below.
        let certs: Option<ServiceCerts> = if use_tls {
            let certs_dir =
                std::env::var("CERTS_DIR").unwrap_or_else(|_| "/certs/agent".to_string());
            Some(ServiceCerts::load(&certs_dir)?)
        } else {
            None
        };

        // Initialize global networking (bridge, forwarding, NAT)
        if let Err(e) = self.firecracker.init_network().await {
            tracing::error!(
                "Failed to initialize host networking: {}. VMs might not have internet access.",
                e
            );
        }

        // Cleanup any stale resources from previous runs
        self.firecracker.cleanup_all_stale_resources().await;

        let certs_for_task = certs.clone();
        let scheduler_addr_for_task = self.scheduler_addr.clone();

        tokio::spawn(async move {
            let mut scheduler_addr = scheduler_addr_for_task;

            // With mTLS the H2 `:scheme` pseudo-header must be "https".
            // tonic derives the scheme from the URI, so switch to https:// here.
            if certs_for_task.is_some() && scheduler_addr.starts_with("http://") {
                scheduler_addr = scheduler_addr.replacen("http://", "https://", 1);
            }

            // Helper: build a fresh Endpoint (+ optional TLS config) for each call.
            // We reconnect on every RPC; acceptable for low-frequency heartbeats.
            let make_endpoint = |addr: &str,
                                 certs: &Option<ServiceCerts>|
             -> Result<
                tonic::transport::Endpoint,
                Box<dyn std::error::Error + Send + Sync>,
            > {
                let ep = tonic::transport::Endpoint::new(addr.to_owned())?;
                match certs {
                    Some(c) => Ok(ep.tls_config(c.client_tls_config("mikrom-scheduler"))?),
                    None => Ok(ep),
                }
            };

            // ── Registration with retry/backoff ───────────────────────────────
            // The scheduler may not be ready yet when this task first runs.
            let register_req = RegisterWorkerRequest {
                host_id: host_id.clone(),
                hostname: hostname.clone(),
                ip_address: ip_address.clone(),
                agent_port: agent_port.into(),
            };
            let mut backoff_secs = 1u64;
            for attempt in 1_u32.. {
                tokio::time::sleep(tokio::time::Duration::from_secs(backoff_secs)).await;

                let result: Result<_, Box<dyn std::error::Error + Send + Sync>> = (async {
                    let ep = make_endpoint(&scheduler_addr, &certs_for_task)?;
                    let channel = ep.connect().await?;
                    let mut client = SchedulerServiceClient::new(channel);
                    Ok(client.register_worker(register_req.clone()).await?)
                })
                .await;

                match result {
                    Ok(resp) => {
                        tracing::info!("Registered with scheduler: {}", resp.into_inner().success);
                        break;
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Registration attempt {attempt} failed: {e:?}. Retrying in {backoff_secs}s..."
                        );
                        backoff_secs = std::cmp::min(backoff_secs * 2, 30);
                    }
                }
            }

            // ── Metrics heartbeat ─────────────────────────────────────────────
            // Report immediately after registration so the scheduler sees this
            // worker as available without waiting for the first 5-second tick.
            loop {
                let metrics = metrics_collector.collect().await;
                tracing::info!(
                    "Collected metrics: cpu={:.2} ram={}/{} disk={}/{}",
                    metrics.cpu_usage,
                    metrics.ram_used_bytes,
                    metrics.ram_total_bytes,
                    metrics.disk_used_bytes,
                    metrics.disk_total_bytes,
                );

                match make_endpoint(&scheduler_addr, &certs_for_task) {
                    Ok(ep) => match ep.connect().await {
                        Ok(channel) => {
                            let mut client = SchedulerServiceClient::new(channel);
                            let req = ReportMetricsRequest {
                                host_id: host_id.clone(),
                                cpu_usage: metrics.cpu_usage,
                                ram_used_bytes: metrics.ram_used_bytes,
                                ram_total_bytes: metrics.ram_total_bytes,
                                disk_used_bytes: metrics.disk_used_bytes,
                                disk_total_bytes: metrics.disk_total_bytes,
                                apps_count: metrics.apps_count,
                                timestamp: metrics.timestamp,
                                load_avg_1: metrics.load_avg_1,
                                load_avg_5: metrics.load_avg_5,
                                load_avg_15: metrics.load_avg_15,
                                vms: metrics
                                    .vms
                                    .into_iter()
                                    .map(|(id, m)| {
                                        (
                                            id,
                                            mikrom_proto::scheduler::VmMetrics {
                                                cpu_usage: m.cpu_usage,
                                                ram_used_bytes: m.ram_used_bytes,
                                            },
                                        )
                                    })
                                    .collect(),
                            };
                            match client.report_metrics(req).await {
                                Ok(resp) => tracing::info!(
                                    "Metrics reported: {}",
                                    resp.into_inner().success
                                ),
                                Err(e) => tracing::error!("Failed to report metrics: {}", e),
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to connect to scheduler for metrics: {}", e)
                        }
                    },
                    Err(e) => tracing::error!("Failed to build scheduler endpoint: {}", e),
                }

                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        });

        // ── gRPC server ───────────────────────────────────────────────────────
        let service = AgentServiceServer::new(self.clone());

        match certs {
            Some(c) => {
                let tls = c.server_tls_config()?;
                tracing::info!("Agent mTLS enabled");
                tonic::transport::Server::builder()
                    .tls_config(tls)?
                    .add_service(service)
                    .serve(addr)
                    .await?;
            }
            None => {
                tracing::info!("Agent running without TLS");
                tonic::transport::Server::builder()
                    .add_service(service)
                    .serve(addr)
                    .await?;
            }
        }

        Ok(())
    }
}

impl Clone for AgentServer {
    fn clone(&self) -> Self {
        Self {
            host_id: self.host_id.clone(),
            hostname: self.hostname.clone(),
            ip_address: self.ip_address.clone(),
            metrics_collector: self.metrics_collector.clone(),
            firecracker: self.firecracker.clone(),
            scheduler_client: self.scheduler_client.clone(),
            shutdown_flag: self.shutdown_flag.clone(),
            scheduler_addr: self.scheduler_addr.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mikrom_proto::agent::{
        GetMetricsRequest, GetVmStatusRequest, MetricsRequest, RegisterRequest, StartVmRequest,
        StopVmRequest, UnregisterRequest,
    };
    use tonic::Request;

    fn make_server() -> AgentServer {
        AgentServer::new(
            "host-1".to_string(),
            "node-1".to_string(),
            "127.0.0.1".to_string(),
        )
    }

    fn start_vm_req(vm_id: &str) -> StartVmRequest {
        StartVmRequest {
            vm_id: vm_id.to_string(),
            app_id: "app-1".to_string(),
            image: "nginx:latest".to_string(),
            config: None,
        }
    }

    #[tokio::test]
    async fn test_register_returns_success() {
        let server = make_server();
        let resp = server
            .register(Request::new(RegisterRequest {
                host_id: "host-1".to_string(),
                hostname: "node-1".to_string(),
                ip_address: "127.0.0.1".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.success);
        assert!(!resp.message.is_empty());
    }

    #[tokio::test]
    async fn test_unregister_returns_success_and_sets_shutdown_flag() {
        let server = make_server();
        assert!(!*server.shutdown_flag.read());
        let resp = server
            .unregister(Request::new(UnregisterRequest {
                host_id: "host-1".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.success);
        assert!(*server.shutdown_flag.read());
    }

    #[tokio::test]
    async fn test_report_metrics_returns_success() {
        let server = make_server();
        let resp = server
            .report_metrics(Request::new(MetricsRequest {
                host_id: "host-1".to_string(),
                cpu_usage: 0.42,
                ram_used_bytes: 512 * 1024 * 1024,
                ram_total_bytes: 4 * 1024 * 1024 * 1024,
                disk_used_bytes: 10 * 1024 * 1024 * 1024,
                disk_total_bytes: 100 * 1024 * 1024 * 1024,
                apps_count: 3,
                timestamp: 1_700_000_000,
                load_avg_1: 0.1,
                load_avg_5: 0.2,
                load_avg_15: 0.3,
                vms: Default::default(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.success);
    }

    #[tokio::test]
    async fn test_get_metrics_returns_correct_host_id() {
        let server = make_server();
        let resp = server
            .get_metrics(Request::new(GetMetricsRequest {
                host_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp.host_id, "host-1");
    }

    #[tokio::test]
    async fn test_get_metrics_real_system_data() {
        let server = make_server();
        let resp = server
            .get_metrics(Request::new(GetMetricsRequest {
                host_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.ram_total_bytes > 0);
        assert!(resp.timestamp > 0);
    }

    #[tokio::test]
    async fn test_get_metrics_initial_apps_count_is_zero() {
        let server = make_server();
        let resp = server
            .get_metrics(Request::new(GetMetricsRequest {
                host_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp.apps_count, 0);
    }

    #[tokio::test]
    async fn test_start_vm_with_explicit_id() {
        let server = make_server();
        let resp = server
            .start_vm(Request::new(StartVmRequest {
                vm_id: "vm-explicit".to_string(),
                app_id: "app-1".to_string(),
                image: "nginx:latest".to_string(),
                config: None,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.success);
        assert_eq!(resp.vm_id, "vm-explicit");
    }

    #[tokio::test]
    async fn test_start_vm_generates_uuid_when_id_is_empty() {
        let server = make_server();
        let resp = server
            .start_vm(Request::new(StartVmRequest {
                vm_id: String::new(),
                app_id: "app-1".to_string(),
                image: "alpine:3".to_string(),
                config: None,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.success);
        assert!(!resp.vm_id.is_empty());
        // UUID has 36 chars
        assert_eq!(resp.vm_id.len(), 36);
    }

    #[tokio::test]
    async fn test_start_vm_uses_defaults_when_config_is_none() {
        let server = make_server();
        server
            .start_vm(Request::new(start_vm_req("vm-def")))
            .await
            .unwrap();
        let vm = server.firecracker.get_vm("vm-def").await.unwrap();
        assert_eq!(vm.config.vcpus, 1);
        assert_eq!(vm.config.memory_mib, 256);
        assert_eq!(vm.config.disk_mib, 1024);
        assert!(vm.config.env.is_empty());
    }

    #[tokio::test]
    async fn test_start_vm_uses_provided_config() {
        let mut env = std::collections::HashMap::new();
        env.insert("PORT".to_string(), "8080".to_string());
        let server = make_server();
        server
            .start_vm(Request::new(StartVmRequest {
                vm_id: "vm-cfg".to_string(),
                app_id: "app-1".to_string(),
                image: "ubuntu:24.04".to_string(),
                config: Some(mikrom_proto::agent::VmConfig {
                    vcpus: 1,
                    memory_mib: 256,
                    disk_mib: 1024,
                    env,
                    ip_address: String::new(),
                    gateway: String::new(),
                    mac_address: String::new(),
                    volumes: vec![],
                }),
            }))
            .await
            .unwrap();
        let vm = server.firecracker.get_vm("vm-cfg").await.unwrap();
        assert_eq!(vm.config.vcpus, 1);
        assert_eq!(vm.config.memory_mib, 256);
        assert_eq!(vm.config.disk_mib, 1024);
        assert_eq!(vm.config.env.get("PORT").map(|s| s.as_str()), Some("8080"));
    }

    #[tokio::test]
    async fn test_start_vm_duplicate_id_returns_failure() {
        let server = make_server();
        server
            .start_vm(Request::new(start_vm_req("vm-dup")))
            .await
            .unwrap();
        let resp = server
            .start_vm(Request::new(start_vm_req("vm-dup")))
            .await
            .unwrap()
            .into_inner();
        assert!(!resp.success);
        assert!(resp.vm_id.is_empty());
        assert!(!resp.message.is_empty());
    }

    #[tokio::test]
    async fn test_start_vm_increments_app_count() {
        let server = make_server();
        server
            .start_vm(Request::new(start_vm_req("vm-cnt")))
            .await
            .unwrap();
        let metrics = server
            .get_metrics(Request::new(GetMetricsRequest {
                host_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(metrics.apps_count, 1);
    }

    #[tokio::test]
    async fn test_stop_vm_success() {
        let server = make_server();
        server
            .start_vm(Request::new(start_vm_req("vm-stop")))
            .await
            .unwrap();
        let resp = server
            .stop_vm(Request::new(StopVmRequest {
                vm_id: "vm-stop".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.success);
    }

    #[tokio::test]
    async fn test_stop_vm_decrements_app_count() {
        let server = make_server();
        server
            .start_vm(Request::new(start_vm_req("vm-dec")))
            .await
            .unwrap();
        server
            .stop_vm(Request::new(StopVmRequest {
                vm_id: "vm-dec".to_string(),
            }))
            .await
            .unwrap();
        let metrics = server
            .get_metrics(Request::new(GetMetricsRequest {
                host_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(metrics.apps_count, 0);
    }

    #[tokio::test]
    async fn test_stop_vm_nonexistent_returns_failure() {
        let server = make_server();
        let resp = server
            .stop_vm(Request::new(StopVmRequest {
                vm_id: "ghost-vm".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!resp.success);
        assert!(resp.message.contains("ghost-vm"));
    }

    #[tokio::test]
    async fn test_get_vm_status_starting() {
        let server = make_server();
        server
            .start_vm(Request::new(start_vm_req("vm-st")))
            .await
            .unwrap();
        let resp = server
            .get_vm_status(Request::new(GetVmStatusRequest {
                vm_id: "vm-st".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp.status, 1); // Starting
        assert_eq!(resp.vm_id, "vm-st");
    }

    #[tokio::test]
    async fn test_get_vm_status_stopping() {
        let server = make_server();
        server
            .start_vm(Request::new(start_vm_req("vm-stp")))
            .await
            .unwrap();
        server
            .stop_vm(Request::new(StopVmRequest {
                vm_id: "vm-stp".to_string(),
            }))
            .await
            .unwrap();
        let resp = server
            .get_vm_status(Request::new(GetVmStatusRequest {
                vm_id: "vm-stp".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp.status, 3); // Stopping
    }

    #[tokio::test]
    async fn test_get_vm_status_running() {
        let server = make_server();
        server
            .start_vm(Request::new(start_vm_req("vm-run")))
            .await
            .unwrap();
        server
            .firecracker
            .set_status_for_test("vm-run", crate::firecracker::VmStatus::Running)
            .await;
        let resp = server
            .get_vm_status(Request::new(GetVmStatusRequest {
                vm_id: "vm-run".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp.status, 2); // Running
    }

    #[tokio::test]
    async fn test_get_vm_status_stopped() {
        let server = make_server();
        server
            .start_vm(Request::new(start_vm_req("vm-stpd")))
            .await
            .unwrap();
        server
            .firecracker
            .set_status_for_test("vm-stpd", crate::firecracker::VmStatus::Stopped)
            .await;
        let resp = server
            .get_vm_status(Request::new(GetVmStatusRequest {
                vm_id: "vm-stpd".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp.status, 4); // Stopped
    }

    #[tokio::test]
    async fn test_get_vm_status_failed() {
        let server = make_server();
        server
            .start_vm(Request::new(start_vm_req("vm-fail")))
            .await
            .unwrap();
        server
            .firecracker
            .set_status_for_test("vm-fail", crate::firecracker::VmStatus::Failed)
            .await;
        let resp = server
            .get_vm_status(Request::new(GetVmStatusRequest {
                vm_id: "vm-fail".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp.status, 5); // Failed
    }

    #[tokio::test]
    async fn test_get_vm_status_nonexistent_returns_not_found() {
        let server = make_server();
        let result = server
            .get_vm_status(Request::new(GetVmStatusRequest {
                vm_id: "ghost".to_string(),
            }))
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn test_clone_shares_firecracker_and_metrics_state() {
        let original = make_server();
        original
            .start_vm(Request::new(start_vm_req("shared-vm")))
            .await
            .unwrap();
        let cloned = original.clone();
        // Clone sees VM started by original (Arc is shared)
        let resp = cloned
            .get_vm_status(Request::new(GetVmStatusRequest {
                vm_id: "shared-vm".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp.status, 1); // Starting
        // Metrics state (apps_count) is also shared
        let metrics = cloned
            .get_metrics(Request::new(GetMetricsRequest {
                host_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(metrics.apps_count, 1);
    }

    #[tokio::test]
    async fn test_clone_shares_shutdown_flag() {
        let original = make_server();
        let cloned = original.clone();
        cloned
            .unregister(Request::new(UnregisterRequest {
                host_id: "host-1".to_string(),
            }))
            .await
            .unwrap();
        // Original sees the flag set by clone
        assert!(*original.shutdown_flag.read());
    }
}
