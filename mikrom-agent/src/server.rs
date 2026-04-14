use crate::metrics::{MetricsCollector, SystemMetrics};
use crate::firecracker::{FirecrackerManager, VmConfig};
use mikrom_proto::agent::{
    RegisterRequest, RegisterResponse, UnregisterRequest, UnregisterResponse,
    MetricsRequest, MetricsResponse, GetMetricsRequest, GetMetricsResponse,
    StartVmRequest, StartVmResponse, StopVmRequest, StopVmResponse,
    GetVmStatusRequest, GetVmStatusResponse, VmStatus,
    agent_service_server::{AgentService, AgentServiceServer},
};
use mikrom_proto::scheduler::{RegisterWorkerRequest, ReportMetricsRequest, SchedulerServiceClient};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tonic::{async_trait, Response, Status};
use uuid::Uuid;

pub struct AgentServer {
    host_id: String,
    hostname: String,
    ip_address: String,
    metrics_collector: MetricsCollector,
    firecracker: FirecrackerManager,
    scheduler_client: Option<SchedulerClient>,
    shutdown_flag: Arc<RwLock<bool>>,
}

#[derive(Clone)]
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

        let metrics = SystemMetrics {
            cpu_usage: req.cpu_usage,
            ram_used_bytes: req.ram_used_bytes,
            ram_total_bytes: req.ram_total_bytes,
            disk_used_bytes: req.disk_used_bytes,
            disk_total_bytes: req.disk_total_bytes,
            apps_count: req.apps_count,
            timestamp: req.timestamp,
        };

        tracing::debug!("Reported metrics: cpu={:.2}, ram={}/{}",
            metrics.cpu_usage, metrics.ram_used_bytes, metrics.ram_total_bytes);

        Ok(Response::new(MetricsResponse { success: true }))
    }

    async fn get_metrics(
        &self,
        _request: tonic::Request<GetMetricsRequest>,
    ) -> Result<Response<GetMetricsResponse>, Status> {
        let metrics = self.metrics_collector.collect();

        Ok(Response::new(GetMetricsResponse {
            host_id: self.host_id.clone(),
            cpu_usage: metrics.cpu_usage,
            ram_used_bytes: metrics.ram_used_bytes,
            ram_total_bytes: metrics.ram_total_bytes,
            disk_used_bytes: metrics.disk_used_bytes,
            disk_total_bytes: metrics.disk_total_bytes,
            apps_count: metrics.apps_count,
            timestamp: metrics.timestamp,
        }))
    }

    async fn start_vm(
        &self,
        request: tonic::Request<StartVmRequest>,
    ) -> Result<Response<StartVmResponse>, Status> {
        let req = request.into_inner();
        
        let vm_id = if req.vm_id.is_empty() {
            Uuid::new_v4().to_string()
        } else {
            req.vm_id.clone()
        };

        let config = VmConfig {
            vcpus: req.config.as_ref().map(|c| c.vcpus).unwrap_or(1),
            memory_mib: req.config.as_ref().map(|c| c.memory_mib).unwrap_or(256),
            disk_mib: req.config.as_ref().map(|c| c.disk_mib).unwrap_or(1024),
            env: req.config.as_ref().map(|c| c.env.clone()).unwrap_or_default(),
        };

        match self.firecracker.start_vm(vm_id.clone(), req.app_id, req.image, config) {
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
        
        match self.firecracker.stop_vm(&req.vm_id) {
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

    async fn get_vm_status(
        &self,
        request: tonic::Request<GetVmStatusRequest>,
    ) -> Result<Response<GetVmStatusResponse>, Status> {
        let req = request.into_inner();
        
        match self.firecracker.get_vm_status(&req.vm_id) {
            Ok(status) => {
                let proto_status = match status {
                    crate::firecracker::VmStatus::Starting => 1,
                    crate::firecracker::VmStatus::Running => 2,
                    crate::firecracker::VmStatus::Stopping => 3,
                    crate::firecracker::VmStatus::Stopped => 4,
                    crate::firecracker::VmStatus::Failed => 5,
                    _ => 0,
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
        Self {
            host_id,
            hostname,
            ip_address,
            metrics_collector: MetricsCollector::new(),
            firecracker: FirecrackerManager::new(),
            scheduler_client: None,
            shutdown_flag: Arc::new(RwLock::new(false)),
        }
    }

    pub async fn serve(&self, addr: SocketAddr, use_tls: bool) -> Result<(), Box<dyn std::error::Error>> {
        let host_id = self.host_id.clone();
        let hostname = self.hostname.clone();
        let ip_address = self.ip_address.clone();
        
        let metrics_collector = self.metrics_collector.clone();
        
        tokio::spawn(async move {
            let host_id = host_id;
            let hostname = hostname;
            let ip_address = ip_address;
            
            let scheduler_addr = std::env::var("SCHEDULER_ADDR")
                .unwrap_or_else(|_| "http://127.0.0.1:5002".to_string());
            
            let static_addr: &'static str = Box::leak(scheduler_addr.into_boxed_str());
            
            let endpoint = match tonic::transport::Endpoint::new(static_addr) {
                Ok(e) => e,
                Err(e) => {
                    tracing::error!("Failed to create scheduler endpoint: {}", e);
                    return;
                }
            };
            
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            
            match SchedulerServiceClient::connect(endpoint.clone()).await {
                Ok(mut client) => {
                    let req = RegisterWorkerRequest {
                        host_id: host_id.clone(),
                        hostname: hostname.clone(),
                        ip_address: ip_address.clone(),
                        agent_port: 5003,
                    };
                    match client.register_worker(req).await {
                        Ok(resp) => {
                            tracing::info!("Registered with scheduler: {}", resp.into_inner().success);
                        }
                        Err(e) => {
                            tracing::error!("Failed to register: {}", e);
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to connect to scheduler for registration: {}", e);
                }
            }
            
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                
                let metrics = metrics_collector.collect();
                tracing::info!("Collected metrics: cpu={:.2} ram={}/{}", 
                    metrics.cpu_usage, metrics.ram_used_bytes, metrics.ram_total_bytes);
                
                match SchedulerServiceClient::connect(endpoint.clone()).await {
                    Ok(mut client) => {
                        let req = ReportMetricsRequest {
                            host_id: host_id.clone(),
                            cpu_usage: metrics.cpu_usage,
                            ram_used_bytes: metrics.ram_used_bytes,
                            ram_total_bytes: metrics.ram_total_bytes,
                            disk_used_bytes: metrics.disk_used_bytes,
                            disk_total_bytes: metrics.disk_total_bytes,
                            apps_count: metrics.apps_count,
                            timestamp: metrics.timestamp,
                        };
                        match client.report_metrics(req).await {
                            Ok(resp) => {
                                tracing::info!("Metrics reported: {}", resp.into_inner().success);
                            }
                            Err(e) => {
                                tracing::error!("Failed to report metrics: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to connect to scheduler for metrics: {}", e);
                    }
                }
            }
        });

        let service = AgentServiceServer::new(self.clone());
        
        if use_tls {
            let tls_config = mikrom_proto::tls::TlsConfig::load_or_generate(&self.host_id, "./certs/agent")?;
            if let Some(tls) = tls_config.create_server_tls_config() {
                tracing::info!("Agent TLS enabled");
                tonic::transport::Server::builder()
                    .tls_config(tls)?
                    .add_service(service)
                    .serve(addr)
                    .await?;
            } else {
                tracing::warn!("Agent TLS failed to configure, using insecure");
                tonic::transport::Server::builder()
                    .add_service(service)
                    .serve(addr)
                    .await?;
            }
        } else {
            tracing::info!("Agent running without TLS");
            tonic::transport::Server::builder()
                .add_service(service)
                .serve(addr)
                .await?;
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
        }
    }
}