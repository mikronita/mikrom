use crate::scheduler::AppScheduler;
use crate::worker_registry::WorkerRegistry;
use crate::metrics::HostMetrics;
use mikrom_proto::scheduler::{
    DeployRequest, DeployResponse, AppStatusRequest, AppStatusResponse,
    CancelRequest, CancelResponse, ListAppsRequest, ListAppsResponse, AppInfo,
    RegisterWorkerRequest, RegisterWorkerResponse,
    ReportMetricsRequest, ReportMetricsResponse,
    scheduler_service_server::{SchedulerService, SchedulerServiceServer},
};
use mikrom_proto::agent::{StartVmRequest, StartVmResponse, agent_service_client::AgentServiceClient};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tonic::{async_trait, Response, Status};
use uuid::Uuid;

pub struct SchedulerServer {
    scheduler: AppScheduler,
    agent_clients: Arc<RwLock<HashMap<String, AgentClient>>>,
}

#[derive(Clone)]
struct AgentClient {
    host_id: String,
    channel: tonic::transport::Channel,
}

#[async_trait]
impl SchedulerService for SchedulerServer {
    async fn register_worker(
        &self,
        request: tonic::Request<RegisterWorkerRequest>,
    ) -> Result<Response<RegisterWorkerResponse>, Status> {
        let req = request.into_inner();
        
        tracing::info!("Registering worker: {} ({}) on {}:{}",
            req.hostname, req.host_id, req.ip_address, req.agent_port);

        let success = self.scheduler.worker_registry().register(
            req.host_id,
            req.hostname,
            req.ip_address,
            req.agent_port as u16,
        );

        Ok(Response::new(RegisterWorkerResponse {
            success,
            message: if success { "Registered".to_string() } else { "Failed".to_string() },
        }))
    }

    async fn report_metrics(
        &self,
        request: tonic::Request<ReportMetricsRequest>,
    ) -> Result<Response<ReportMetricsResponse>, Status> {
        let req = request.into_inner();
        
        let metrics = HostMetrics {
            cpu_usage: req.cpu_usage,
            ram_used_bytes: req.ram_used_bytes,
            ram_total_bytes: req.ram_total_bytes,
            disk_used_bytes: req.disk_used_bytes,
            disk_total_bytes: req.disk_total_bytes,
            apps_count: req.apps_count,
            timestamp: req.timestamp,
        };
        
        let success = self.scheduler.worker_registry().update_metrics(&req.host_id, metrics.clone());
        
        if success {
            tracing::info!("Updated metrics for worker {}: cpu={:.2} ram={}/{}", 
                req.host_id, metrics.cpu_usage, metrics.ram_used_bytes, metrics.ram_total_bytes);
        } else {
            tracing::warn!("Failed to update metrics for worker {}", req.host_id);
        }

        Ok(Response::new(ReportMetricsResponse { success }))
    }

    async fn deploy_app(
        &self,
        request: tonic::Request<DeployRequest>,
    ) -> Result<Response<DeployResponse>, Status> {
        let req = request.into_inner();
        
        let job_id = Uuid::new_v4().to_string();
        let vm_id = Uuid::new_v4().to_string();

        let config = crate::job::VmConfig {
            vcpus: req.config.as_ref().map(|c| c.vcpus).unwrap_or(1),
            memory_mib: req.config.as_ref().map(|c| c.memory_mib).unwrap_or(256),
            disk_mib: req.config.as_ref().map(|c| c.disk_mib).unwrap_or(1024),
            env: req.config.as_ref().map(|c| c.env.clone()).unwrap_or_default(),
        };

        let result = self.scheduler.select_best_worker(&config);

        let response = match result {
            Ok(worker) => {
                let app_id = req.app_id.clone();
                let image = req.image.clone();
                let host_id = worker.host_id.clone();
                
                let mut job = crate::job::Job::new(
                    job_id.clone(),
                    req.app_id,
                    req.app_name,
                    req.image,
                    config.clone(),
                    req.user_id,
                );
                job.schedule(host_id.clone(), vm_id.clone());
                self.scheduler.add_job(job);

                let _ = self.forward_deploy_to_agent(
                    &host_id,
                    &app_id,
                    &image,
                    &vm_id,
                    &config,
                ).await;

                DeployResponse {
                    job_id: job_id.clone(),
                    status: crate::job::JobStatus::Scheduled as i32,
                    host_id,
                    vm_id,
                    message: "Application scheduled".to_string(),
                }
            }
            Err(e) => {
                let mut job = crate::job::Job::new(
                    job_id.clone(),
                    req.app_id,
                    req.app_name,
                    req.image,
                    config,
                    req.user_id,
                );
                job.fail(e.to_string());
                self.scheduler.add_job(job);

                DeployResponse {
                    job_id: job_id.clone(),
                    status: crate::job::JobStatus::Failed as i32,
                    host_id: String::new(),
                    vm_id: String::new(),
                    message: e.to_string(),
                }
            }
        };

        Ok(Response::new(response))
    }

    async fn get_app_status(
        &self,
        request: tonic::Request<AppStatusRequest>,
    ) -> Result<Response<AppStatusResponse>, Status> {
        let req = request.into_inner();
        
        match self.scheduler.get_job(&req.job_id) {
            Some(job) => {
                let response = AppStatusResponse {
                    job_id: job.job_id,
                    status: job.status as i32,
                    host_id: job.host_id.unwrap_or_default(),
                    vm_id: job.vm_id.unwrap_or_default(),
                    scheduled_at: job.scheduled_at.unwrap_or(0),
                    started_at: job.started_at.unwrap_or(0),
                    stopped_at: job.stopped_at.unwrap_or(0),
                    error_message: job.error_message.unwrap_or_default(),
                };
                Ok(Response::new(response))
            }
            None => Err(Status::not_found("Job not found")),
        }
    }

    async fn cancel_app(
        &self,
        request: tonic::Request<CancelRequest>,
    ) -> Result<Response<CancelResponse>, Status> {
        let req = request.into_inner();
        
        if let Some(mut job) = self.scheduler.get_job(&req.job_id) {
            job.cancel();
            self.scheduler.update_job_status(&req.job_id, job.status);

            if let Some(vm_id) = &job.vm_id {
                let _ = self.stop_vm_on_agent(job.host_id.as_deref().unwrap_or(""), vm_id).await;
            }

            Ok(Response::new(CancelResponse {
                success: true,
                message: "Application cancelled".to_string(),
            }))
        } else {
            Ok(Response::new(CancelResponse {
                success: false,
                message: "Job not found".to_string(),
            }))
        }
    }

    async fn list_apps(
        &self,
        request: tonic::Request<ListAppsRequest>,
    ) -> Result<Response<ListAppsResponse>, Status> {
        let req = request.into_inner();
        
        let jobs = self.scheduler.list_jobs(Some(req.user_id.as_str()), None);
        
        let apps = jobs
            .into_iter()
            .map(|j| AppInfo {
                job_id: j.job_id,
                app_id: j.app_id,
                app_name: j.app_name,
                image: j.image,
                status: j.status as i32,
                host_id: j.host_id.unwrap_or_default(),
                vm_id: j.vm_id.unwrap_or_default(),
            })
            .collect();

        Ok(Response::new(ListAppsResponse { apps }))
    }
}

impl SchedulerServer {
    pub fn new(_addr: SocketAddr) -> Result<Self, Box<dyn std::error::Error>> {
        let worker_registry = WorkerRegistry::new();
        let scheduler = AppScheduler::new(worker_registry);

        Ok(Self {
            scheduler,
            agent_clients: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub async fn serve(&self, use_tls: bool) -> Result<(), Box<dyn std::error::Error>> {
        let addr: SocketAddr = "0.0.0.0:5002".parse()?;
        
        let service = SchedulerServiceServer::new(self.clone());
        
        if use_tls {
            let tls_config = mikrom_proto::tls::TlsConfig::load_or_generate("scheduler", "./certs/scheduler")?;
            if let Some(tls) = tls_config.create_server_tls_config() {
                tracing::info!("Scheduler TLS enabled");
                tonic::transport::Server::builder()
                    .tls_config(tls)?
                    .add_service(service)
                    .serve(addr)
                    .await?;
            } else {
                tracing::warn!("Scheduler TLS failed to configure, using insecure");
                tonic::transport::Server::builder()
                    .add_service(service)
                    .serve(addr)
                    .await?;
            }
        } else {
            tracing::info!("Scheduler running without TLS");
            tonic::transport::Server::builder()
                .add_service(service)
.serve(addr)
            .await?;
        }
        
        Ok(())
    }

    fn get_agent_client(&self, host_id: &str) -> Option<AgentClient> {
        self.agent_clients.read().get(host_id).cloned()
    }

    async fn forward_deploy_to_agent(
        &self,
        host_id: &str,
        app_id: &str,
        image: &str,
        vm_id: &str,
        config: &crate::job::VmConfig,
    ) -> Result<(), Status> {
        let worker = self.scheduler.worker_registry().get_worker(host_id)
            .ok_or_else(|| Status::not_found("Worker not found"))?;
        
        let addr = format!("http://{}:{}", worker.ip_address, worker.agent_port);
        let static_addr: &'static str = Box::leak(addr.into_boxed_str());
        
        let endpoint = tonic::transport::Endpoint::new(static_addr)
            .map_err(|e| Status::unavailable(format!("Failed to create endpoint: {}", e)))?;
        
        let mut client = AgentServiceClient::connect(endpoint)
            .await
            .map_err(|e| Status::unavailable(format!("Failed to connect to agent: {}", e)))?;
        
        let req = StartVmRequest {
            vm_id: vm_id.to_string(),
            app_id: app_id.to_string(),
            image: image.to_string(),
            config: Some(mikrom_proto::agent::VmConfig {
                vcpus: config.vcpus,
                memory_mib: config.memory_mib,
                disk_mib: config.disk_mib,
                env: config.env.clone(),
            }),
        };
        
        let resp = client.start_vm(req).await
            .map_err(|e| Status::internal(format!("Failed to start VM: {}", e)))?
            .into_inner();
        
        if resp.success {
            tracing::info!("VM {} started on host {}", vm_id, host_id);
            Ok(())
        } else {
            tracing::error!("Failed to start VM: {}", resp.message);
            Err(Status::internal(resp.message))
        }
    }

    async fn stop_vm_on_agent(&self, _host_id: &str, _vm_id: &str) -> Result<(), Status> {
        Ok(())
    }
}

impl Clone for SchedulerServer {
    fn clone(&self) -> Self {
        Self {
            scheduler: AppScheduler::new(self.scheduler.worker_registry().clone()),
            agent_clients: self.agent_clients.clone(),
        }
    }
}