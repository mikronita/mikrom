use crate::metrics::HostMetrics;
use crate::scheduler::AppScheduler;
use crate::worker_registry::WorkerRegistry;
use mikrom_proto::agent::{StartVmRequest, agent_service_client::AgentServiceClient};
use mikrom_proto::scheduler::{
    AppInfo, AppStatusRequest, AppStatusResponse, CancelRequest, CancelResponse, DeleteAppRequest,
    DeleteAppResponse, DeployRequest, DeployResponse, GetLogsRequest, GetLogsResponse,
    ListAppsRequest, ListAppsResponse, PauseRequest, PauseResponse, RegisterWorkerRequest,
    RegisterWorkerResponse, ReportMetricsRequest, ReportMetricsResponse, ResumeRequest,
    ResumeResponse,
    scheduler_service_server::{SchedulerService, SchedulerServiceServer},
};

use mikrom_proto::tls::ServiceCerts;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tonic::{Response, Status, async_trait};
use uuid::Uuid;

pub struct SchedulerServer {
    scheduler: AppScheduler,
    agent_clients: Arc<RwLock<HashMap<String, AgentClient>>>,
    certs: Option<ServiceCerts>,
}

#[derive(Clone)]
#[allow(dead_code)]
struct AgentClient {
    host_id: String,
    channel: tonic::transport::Channel,
}

#[async_trait]
impl SchedulerService for SchedulerServer {
    #[tracing::instrument(skip(self, request), fields(host_id = %request.get_ref().host_id))]
    async fn register_worker(
        &self,
        request: tonic::Request<RegisterWorkerRequest>,
    ) -> Result<Response<RegisterWorkerResponse>, Status> {
        let req = request.into_inner();

        tracing::info!(
            "Registering worker: {} ({}) on {}:{}",
            req.hostname,
            req.host_id,
            req.ip_address,
            req.agent_port
        );

        let success = self.scheduler.worker_registry().register(
            req.host_id.clone(),
            req.hostname,
            req.ip_address,
            req.agent_port as u16,
        );

        if !success {
            tracing::error!("Failed to register worker: {}", req.host_id);
        }

        Ok(Response::new(RegisterWorkerResponse {
            success,
            message: if success {
                "Registered".to_string()
            } else {
                "Failed".to_string()
            },
        }))
    }

    #[tracing::instrument(skip(self, request), fields(host_id = %request.get_ref().host_id))]
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
            load_avg_1: req.load_avg_1,
            load_avg_5: req.load_avg_5,
            load_avg_15: req.load_avg_15,
            vms: req
                .vms
                .into_iter()
                .map(|(id, m)| {
                    (
                        id,
                        crate::metrics::VmMetrics {
                            cpu_usage: m.cpu_usage,
                            ram_used_bytes: m.ram_used_bytes,
                        },
                    )
                })
                .collect(),
        };

        let success = self
            .scheduler
            .worker_registry()
            .update_metrics(&req.host_id, metrics.clone());

        if success {
            tracing::info!(
                "Updated metrics for worker {}: cpu={:.2} ram={}/{} disk={}/{}",
                req.host_id,
                metrics.cpu_usage,
                metrics.ram_used_bytes,
                metrics.ram_total_bytes,
                metrics.disk_used_bytes,
                metrics.disk_total_bytes
            );
        } else {
            tracing::warn!("Failed to update metrics for worker {}", req.host_id);
        }

        Ok(Response::new(ReportMetricsResponse { success }))
    }

    #[tracing::instrument(skip(self, request), fields(app_id = %request.get_ref().app_id, user_id = %request.get_ref().user_id))]
    async fn deploy_app(
        &self,
        request: tonic::Request<DeployRequest>,
    ) -> Result<Response<DeployResponse>, Status> {
        let req = request.into_inner();
        tracing::info!(
            app_id = %req.app_id,
            user_id = %req.user_id,
            image = %req.image,
            "Handling deploy_app request"
        );

        let job_id = Uuid::new_v4().to_string();
        let vm_id = Uuid::new_v4().to_string();

        let config = crate::job::VmConfig {
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
            env: req
                .config
                .as_ref()
                .map(|c| c.env.clone())
                .unwrap_or_default(),
            ip_address: None,
            gateway: None,
            mac_address: None,
            volumes: req
                .config
                .as_ref()
                .map(|c| {
                    c.volumes
                        .iter()
                        .map(|v| crate::job::Volume {
                            volume_id: v.volume_id.clone(),
                            size_mib: v.size_mib,
                            read_only: v.read_only,
                        })
                        .collect()
                })
                .unwrap_or_default(),
        };

        let result = self.scheduler.select_best_worker(&config);

        let response = match result {
            Ok(worker) => {
                let app_id = req.app_id.clone();
                let image = req.image.clone();
                let host_id = worker.host_id.clone();

                // Assign networking (Mock implementation: unique IP per job)
                // In real world, use an IPAM (IP Address Management)
                let last_byte = (chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0) % 250) + 2;
                let guest_ip = format!("172.16.1.{}", last_byte);
                let gateway = "172.16.1.1".to_string();
                let mac = format!("AA:BB:CC:01:01:{:02x}", last_byte);

                let job_config = crate::job::VmConfig {
                    vcpus: config.vcpus,
                    memory_mib: config.memory_mib,
                    disk_mib: config.disk_mib,
                    env: config.env.clone(),
                    ip_address: Some(guest_ip),
                    gateway: Some(gateway),
                    mac_address: Some(mac),
                    volumes: config
                        .volumes
                        .iter()
                        .map(|v| crate::job::Volume {
                            volume_id: v.volume_id.clone(),
                            size_mib: v.size_mib,
                            read_only: v.read_only,
                        })
                        .collect(),
                };

                let mut job = crate::job::Job::new(
                    job_id.clone(),
                    req.app_id,
                    req.app_name,
                    req.image,
                    job_config.clone(),
                    req.user_id,
                );
                job.schedule(host_id.clone(), vm_id.clone());
                self.scheduler.add_job(job);

                match self
                    .forward_deploy_to_agent(&host_id, &app_id, &image, &vm_id, &job_config)
                    .await
                {
                    Ok(()) => {
                        self.scheduler.start_job(&job_id);
                    }
                    Err(e) => {
                        self.scheduler.fail_job(&job_id, e.message().to_string());
                    }
                }

                let job = self.scheduler.get_job(&job_id);
                let status = job
                    .as_ref()
                    .map(|j| j.status as i32)
                    .unwrap_or(crate::job::JobStatus::Scheduled as i32);
                let message = job
                    .as_ref()
                    .and_then(|j| j.error_message.clone())
                    .unwrap_or_else(|| "Application scheduled".to_string());

                DeployResponse {
                    job_id: job_id.clone(),
                    status,
                    host_id,
                    vm_id,
                    message,
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

    #[tracing::instrument(skip(self, request), fields(job_id = %request.get_ref().job_id))]
    async fn get_app_status(
        &self,
        request: tonic::Request<AppStatusRequest>,
    ) -> Result<Response<AppStatusResponse>, Status> {
        let req = request.into_inner();
        tracing::debug!("Checking app status");

        match self.scheduler.get_job(&req.job_id) {
            Some(job) if job.user_id == req.user_id => {
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
            Some(_) => {
                tracing::warn!("User {} unauthorized for job {}", req.user_id, req.job_id);
                Err(Status::permission_denied("You do not own this job"))
            }
            None => {
                tracing::warn!("Job {} not found", req.job_id);
                Err(Status::not_found("Job not found"))
            }
        }
    }

    #[allow(clippy::result_large_err)]
    #[tracing::instrument(skip(self, request), fields(job_id = %request.get_ref().job_id))]
    async fn get_app_logs(
        &self,
        request: tonic::Request<GetLogsRequest>,
    ) -> Result<Response<Self::GetAppLogsStream>, Status> {
        let req = request.into_inner();
        let job_id = req.job_id;
        let user_id = req.user_id;

        let job = self.scheduler.get_job(&job_id).ok_or_else(|| {
            tracing::warn!("Job {} not found", job_id);
            Status::not_found("Job not found")
        })?;

        if job.user_id != user_id {
            tracing::warn!("User {} unauthorized for job {}", user_id, job_id);
            return Err(Status::permission_denied("You do not own this job"));
        }

        let host_id = job.host_id.ok_or_else(|| {
            tracing::error!("Job {} has no assigned host", job_id);
            Status::failed_precondition("Job has no assigned host")
        })?;
        let vm_id = job.vm_id.ok_or_else(|| {
            tracing::error!("Job {} has no assigned VM ID", job_id);
            Status::failed_precondition("Job has no assigned VM ID")
        })?;

        let mut client = self.get_agent_client(&host_id).await?;

        let agent_req = mikrom_proto::agent::GetLogsRequest {
            vm_id,
            follow: req.follow,
        };

        let stream = client
            .get_logs(agent_req)
            .await
            .map_err(|e| {
                tracing::error!("Failed to get logs from agent {}: {}", host_id, e);
                e
            })?
            .into_inner();

        let output_stream = tokio_stream::StreamExt::map(stream, |res| {
            res.map(|msg| GetLogsResponse {
                line: msg.line,
                timestamp: msg.timestamp,
            })
        });

        Ok(Response::new(
            Box::pin(output_stream) as Self::GetAppLogsStream
        ))
    }

    type GetAppLogsStream =
        std::pin::Pin<Box<dyn tokio_stream::Stream<Item = Result<GetLogsResponse, Status>> + Send>>;

    #[tracing::instrument(skip(self, request), fields(job_id = %request.get_ref().job_id))]
    async fn cancel_app(
        &self,
        request: tonic::Request<CancelRequest>,
    ) -> Result<Response<CancelResponse>, Status> {
        let req = request.into_inner();
        tracing::info!("Cancelling application");

        if let Some(job) = self.scheduler.get_job(&req.job_id) {
            if job.user_id != req.user_id {
                tracing::warn!("User {} unauthorized for job {}", req.user_id, req.job_id);
                return Err(Status::permission_denied("You do not own this job"));
            }

            self.scheduler.cancel_job(&req.job_id);

            if let Some(vm_id) = &job.vm_id {
                let _ = self
                    .stop_vm_on_agent(job.host_id.as_deref().unwrap_or(""), vm_id)
                    .await;
            }

            Ok(Response::new(CancelResponse {
                success: true,
                message: "Application cancelled".to_string(),
            }))
        } else {
            tracing::warn!("Job {} not found", req.job_id);
            Ok(Response::new(CancelResponse {
                success: false,
                message: "Job not found".to_string(),
            }))
        }
    }

    #[tracing::instrument(skip(self, request), fields(job_id = %request.get_ref().job_id))]
    async fn delete_app(
        &self,
        request: tonic::Request<DeleteAppRequest>,
    ) -> Result<Response<DeleteAppResponse>, Status> {
        let req = request.into_inner();
        tracing::info!("Deleting application");

        if let Some(job) = self.scheduler.get_job(&req.job_id) {
            if job.user_id != req.user_id {
                tracing::warn!("User {} unauthorized for job {}", req.user_id, req.job_id);
                return Err(Status::permission_denied("You do not own this job"));
            }

            // First stop it if it's running
            if let Some(vm_id) = &job.vm_id {
                let _ = self
                    .stop_vm_on_agent(job.host_id.as_deref().unwrap_or(""), vm_id)
                    .await;
            }

            let success = self.scheduler.remove_job(&req.job_id);

            if !success {
                tracing::error!("Failed to delete application {}", req.job_id);
            }

            Ok(Response::new(DeleteAppResponse {
                success,
                message: if success {
                    "Application deleted".to_string()
                } else {
                    "Failed to delete application".to_string()
                },
            }))
        } else {
            tracing::warn!("Job {} not found", req.job_id);
            Ok(Response::new(DeleteAppResponse {
                success: false,
                message: "Job not found".to_string(),
            }))
        }
    }

    #[tracing::instrument(skip(self, request), fields(job_id = %request.get_ref().job_id))]
    async fn pause_app(
        &self,
        request: tonic::Request<PauseRequest>,
    ) -> Result<Response<PauseResponse>, Status> {
        let req = request.into_inner();
        let job = self.scheduler.get_job(&req.job_id).ok_or_else(|| {
            tracing::warn!("Job {} not found", req.job_id);
            Status::not_found("Job not found")
        })?;

        if job.user_id != req.user_id {
            tracing::warn!("User {} unauthorized for job {}", req.user_id, req.job_id);
            return Err(Status::permission_denied("Not your job"));
        }

        let host_id = job.host_id.ok_or_else(|| {
            tracing::error!("Job {} not scheduled", req.job_id);
            Status::failed_precondition("Job not scheduled")
        })?;
        let vm_id = job.vm_id.ok_or_else(|| {
            tracing::error!("Job {} has no VM ID assigned", req.job_id);
            Status::failed_precondition("No VM ID assigned")
        })?;

        let mut client = self.get_agent_client(&host_id).await?;
        let agent_resp = client
            .pause_vm(mikrom_proto::agent::PauseVmRequest { vm_id })
            .await
            .map_err(|e| {
                tracing::error!(
                    "Failed to pause VM {} on host {}: {}",
                    req.job_id,
                    host_id,
                    e
                );
                e
            })?;

        if agent_resp.get_ref().success {
            self.scheduler
                .update_job_status(&req.job_id, crate::job::JobStatus::Paused);
        }

        Ok(Response::new(PauseResponse {
            success: agent_resp.get_ref().success,
            message: agent_resp.get_ref().message.clone(),
        }))
    }

    #[tracing::instrument(skip(self, request), fields(job_id = %request.get_ref().job_id))]
    async fn resume_app(
        &self,
        request: tonic::Request<ResumeRequest>,
    ) -> Result<Response<ResumeResponse>, Status> {
        let req = request.into_inner();
        let job = self.scheduler.get_job(&req.job_id).ok_or_else(|| {
            tracing::warn!("Job {} not found", req.job_id);
            Status::not_found("Job not found")
        })?;

        if job.user_id != req.user_id {
            tracing::warn!("User {} unauthorized for job {}", req.user_id, req.job_id);
            return Err(Status::permission_denied("Not your job"));
        }

        let host_id = job.host_id.ok_or_else(|| {
            tracing::error!("Job {} not scheduled", req.job_id);
            Status::failed_precondition("Job not scheduled")
        })?;
        let vm_id = job.vm_id.ok_or_else(|| {
            tracing::error!("Job {} has no VM ID assigned", req.job_id);
            Status::failed_precondition("No VM ID assigned")
        })?;

        let mut client = self.get_agent_client(&host_id).await?;
        let agent_resp = client
            .resume_vm(mikrom_proto::agent::ResumeVmRequest { vm_id })
            .await
            .map_err(|e| {
                tracing::error!(
                    "Failed to resume VM {} on host {}: {}",
                    req.job_id,
                    host_id,
                    e
                );
                e
            })?;

        if agent_resp.get_ref().success {
            self.scheduler
                .update_job_status(&req.job_id, crate::job::JobStatus::Running);
        }

        Ok(Response::new(ResumeResponse {
            success: agent_resp.get_ref().success,
            message: agent_resp.get_ref().message.clone(),
        }))
    }

    #[tracing::instrument(skip(self, request), fields(user_id = %request.get_ref().user_id))]
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
    pub fn new(certs: Option<ServiceCerts>) -> Result<Self, Box<dyn std::error::Error>> {
        let worker_registry = WorkerRegistry::new();
        let scheduler = AppScheduler::new(worker_registry);

        Ok(Self {
            scheduler,
            agent_clients: Arc::new(RwLock::new(HashMap::new())),
            certs,
        })
    }

    pub async fn serve(&self, addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
        let service = SchedulerServiceServer::new(self.clone());

        match &self.certs {
            Some(certs) => {
                let tls = certs.server_tls_config()?;
                tracing::info!("Scheduler mTLS enabled");
                tonic::transport::Server::builder()
                    .tls_config(tls)?
                    .add_service(service)
                    .serve(addr)
                    .await?;
            }
            None => {
                tracing::info!("Scheduler running without TLS");
                tonic::transport::Server::builder()
                    .add_service(service)
                    .serve(addr)
                    .await?;
            }
        }

        Ok(())
    }

    async fn get_agent_client(
        &self,
        host_id: &str,
    ) -> Result<AgentServiceClient<tonic::transport::Channel>, Status> {
        let worker = self
            .scheduler
            .worker_registry()
            .get_worker(host_id)
            .ok_or_else(|| {
                tracing::warn!("Worker {} not found", host_id);
                Status::not_found(format!("Worker {} not found", host_id))
            })?;

        let (addr, domain) = match &self.certs {
            Some(_) => (
                format!("https://{}:{}", worker.hostname, worker.agent_port),
                worker.hostname.clone(),
            ),
            None => (
                format!("http://{}:{}", worker.ip_address, worker.agent_port),
                String::new(),
            ),
        };

        let mut endpoint = tonic::transport::Endpoint::new(addr)
            .map_err(|e| Status::unavailable(format!("Invalid agent endpoint: {}", e)))?;

        if let Some(certs) = &self.certs {
            endpoint = endpoint
                .tls_config(certs.client_tls_config(&domain))
                .map_err(|e| Status::internal(format!("TLS config error: {}", e)))?;
        }

        let channel = endpoint.connect().await.map_err(|e| {
            tracing::error!("Failed to connect to agent {}: {}", host_id, e);
            Status::unavailable(format!("Failed to connect to agent: {}", e))
        })?;

        Ok(AgentServiceClient::new(channel))
    }

    async fn forward_deploy_to_agent(
        &self,
        host_id: &str,
        app_id: &str,
        image: &str,
        vm_id: &str,
        config: &crate::job::VmConfig,
    ) -> Result<(), Status> {
        let mut client = self.get_agent_client(host_id).await?;

        let req = StartVmRequest {
            vm_id: vm_id.to_string(),
            app_id: app_id.to_string(),
            image: image.to_string(),
            config: Some(mikrom_proto::agent::VmConfig {
                vcpus: config.vcpus,
                memory_mib: config.memory_mib as u32,
                disk_mib: config.disk_mib as u32,
                env: config.env.clone(),
                ip_address: config.ip_address.clone().unwrap_or_default(),
                gateway: config.gateway.clone().unwrap_or_default(),
                mac_address: config.mac_address.clone().unwrap_or_default(),
                volumes: config
                    .volumes
                    .iter()
                    .map(|v| mikrom_proto::agent::Volume {
                        volume_id: v.volume_id.clone(),
                        size_mib: v.size_mib,
                        read_only: v.read_only,
                    })
                    .collect(),
            }),
        };

        let resp = client
            .start_vm(req)
            .await
            .map_err(|e| {
                tracing::error!("Failed to start VM {} on agent {}: {}", vm_id, host_id, e);
                Status::internal(format!("Failed to start VM: {}", e))
            })?
            .into_inner();

        if resp.success {
            tracing::info!("VM {} started on host {}", vm_id, host_id);
            Ok(())
        } else {
            tracing::error!(
                "Agent reported failure starting VM {}: {}",
                vm_id,
                resp.message
            );
            Err(Status::internal(resp.message))
        }
    }

    async fn stop_vm_on_agent(&self, host_id: &str, vm_id: &str) -> Result<(), Status> {
        if host_id.is_empty() {
            return Ok(());
        }

        let mut client = match self.get_agent_client(host_id).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    "stop_vm_on_agent: could not connect to agent {} for vm {}: {}",
                    host_id,
                    vm_id,
                    e
                );
                return Ok(());
            }
        };

        match client
            .stop_vm(mikrom_proto::agent::StopVmRequest {
                vm_id: vm_id.to_string(),
            })
            .await
        {
            Ok(resp) => {
                let inner = resp.into_inner();
                if inner.success {
                    tracing::info!("VM {} stopped on host {}", vm_id, host_id);
                } else {
                    tracing::warn!(
                        "Agent reported failure stopping VM {} on host {}: {}",
                        vm_id,
                        host_id,
                        inner.message
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    "stop_vm_on_agent: RPC error for vm {} on host {}: {}",
                    vm_id,
                    host_id,
                    e.message()
                );
            }
        }

        Ok(())
    }
}

impl Clone for SchedulerServer {
    fn clone(&self) -> Self {
        Self {
            scheduler: self.scheduler.clone(),
            agent_clients: self.agent_clients.clone(),
            certs: self.certs.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mikrom_proto::scheduler::{
        AppConfig, AppStatusRequest, CancelRequest, DeleteAppRequest, DeployRequest,
        ListAppsRequest, RegisterWorkerRequest, ReportMetricsRequest,
    };
    use tonic::Request;

    const GIB: u64 = 1024 * 1024 * 1024;

    fn make_server() -> SchedulerServer {
        SchedulerServer::new(None).unwrap()
    }

    async fn register_worker(server: &SchedulerServer, host_id: &str) {
        server
            .register_worker(Request::new(RegisterWorkerRequest {
                host_id: host_id.to_string(),
                hostname: host_id.to_string(),
                ip_address: "127.0.0.1".to_string(),
                agent_port: 19999,
            }))
            .await
            .unwrap();
    }

    async fn add_metrics(server: &SchedulerServer, host_id: &str) {
        server
            .report_metrics(Request::new(ReportMetricsRequest {
                host_id: host_id.to_string(),
                cpu_usage: 0.1,
                ram_used_bytes: 512 * 1024 * 1024,
                ram_total_bytes: 4 * GIB,
                disk_used_bytes: 10 * GIB,
                disk_total_bytes: 100 * GIB,
                apps_count: 0,
                timestamp: 0,
                load_avg_1: 0.1,
                load_avg_5: 0.2,
                load_avg_15: 0.3,
                vms: Default::default(),
            }))
            .await
            .unwrap();
    }

    fn deploy_req(user_id: &str) -> DeployRequest {
        DeployRequest {
            app_id: "app-1".to_string(),
            app_name: "my-app".to_string(),
            image: "nginx:latest".to_string(),
            config: None,
            user_id: user_id.to_string(),
        }
    }

    #[tokio::test]
    async fn test_register_worker_succeeds() {
        let server = make_server();
        let resp = server
            .register_worker(Request::new(RegisterWorkerRequest {
                host_id: "h1".to_string(),
                hostname: "node-1".to_string(),
                ip_address: "10.0.0.1".to_string(),
                agent_port: 5003,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.success);
        assert!(!resp.message.is_empty());
    }

    #[tokio::test]
    async fn test_register_worker_overwrites_existing() {
        let server = make_server();
        register_worker(&server, "h1").await;
        // Re-registering same host_id should also succeed
        let resp = server
            .register_worker(Request::new(RegisterWorkerRequest {
                host_id: "h1".to_string(),
                hostname: "node-1-v2".to_string(),
                ip_address: "127.0.0.2".to_string(),
                agent_port: 5003,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.success);
    }

    #[tokio::test]
    async fn test_report_metrics_for_registered_worker_succeeds() {
        let server = make_server();
        register_worker(&server, "h1").await;
        let resp = server
            .report_metrics(Request::new(ReportMetricsRequest {
                host_id: "h1".to_string(),
                cpu_usage: 0.5,
                ram_used_bytes: GIB,
                ram_total_bytes: 4 * GIB,
                disk_used_bytes: 20 * GIB,
                disk_total_bytes: 100 * GIB,
                apps_count: 2,
                timestamp: 1_700_000_000,
                load_avg_1: 0.5,
                load_avg_5: 0.4,
                load_avg_15: 0.3,
                vms: Default::default(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.success);
    }

    #[tokio::test]
    async fn test_report_metrics_for_unknown_worker_fails() {
        let server = make_server();
        let resp = server
            .report_metrics(Request::new(ReportMetricsRequest {
                host_id: "ghost".to_string(),
                cpu_usage: 0.1,
                ram_used_bytes: 0,
                ram_total_bytes: 0,
                disk_used_bytes: 0,
                disk_total_bytes: 0,
                apps_count: 0,
                timestamp: 0,
                load_avg_1: 0.0,
                load_avg_5: 0.0,
                load_avg_15: 0.0,
                vms: HashMap::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!resp.success);
    }

    #[tokio::test]
    async fn test_deploy_app_with_no_workers_returns_failed_status() {
        let server = make_server();
        let resp = server
            .deploy_app(Request::new(deploy_req("user-1")))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp.status, crate::job::JobStatus::Failed as i32);
        assert!(resp.host_id.is_empty());
        assert!(resp.vm_id.is_empty());
        assert!(!resp.message.is_empty());
        assert!(!resp.job_id.is_empty());
    }

    #[tokio::test]
    async fn test_deploy_app_with_available_worker_assigns_host_and_vm() {
        // Worker is selected but the agent at port 19999 is unreachable in tests,
        // so the job ends up Failed. The important thing is host and vm are assigned.
        let server = make_server();
        register_worker(&server, "h1").await;
        add_metrics(&server, "h1").await;

        let resp = server
            .deploy_app(Request::new(DeployRequest {
                app_id: "app-1".to_string(),
                app_name: "my-app".to_string(),
                image: "nginx:latest".to_string(),
                config: Some(AppConfig {
                    vcpus: 1,
                    memory_mib: 256,
                    disk_mib: 1024,
                    env: Default::default(),
                    ip_address: "".to_string(),
                    gateway: "".to_string(),
                    mac_address: "".to_string(),
                    volumes: vec![],
                }),
                user_id: "user-1".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        // Agent at port 19999 is unreachable → job transitions to Failed.
        assert_eq!(resp.status, crate::job::JobStatus::Failed as i32);
        assert!(!resp.job_id.is_empty());
        assert_eq!(resp.host_id, "h1");
        assert!(!resp.vm_id.is_empty());
    }

    #[tokio::test]
    async fn test_deploy_app_uses_default_config_when_none() {
        let server = make_server();
        register_worker(&server, "h1").await;
        add_metrics(&server, "h1").await;

        let resp = server
            .deploy_app(Request::new(deploy_req("user-1")))
            .await
            .unwrap()
            .into_inner();
        // Default config fits the worker; agent unreachable → Failed.
        assert_eq!(resp.status, crate::job::JobStatus::Failed as i32);
    }

    #[tokio::test]
    async fn test_deploy_app_job_is_persisted_and_queryable() {
        let server = make_server();
        // Failed deploy is still persisted
        let deploy_resp = server
            .deploy_app(Request::new(deploy_req("user-1")))
            .await
            .unwrap()
            .into_inner();

        let status = server
            .get_app_status(Request::new(AppStatusRequest {
                job_id: deploy_resp.job_id.clone(),
                user_id: "user-1".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(status.job_id, deploy_resp.job_id);
        assert_eq!(status.status, crate::job::JobStatus::Failed as i32);
    }

    #[tokio::test]
    async fn test_get_app_status_scheduled_job_has_host_and_vm() {
        let server = make_server();
        register_worker(&server, "h1").await;
        add_metrics(&server, "h1").await;

        let deploy_resp = server
            .deploy_app(Request::new(deploy_req("user-1")))
            .await
            .unwrap()
            .into_inner();

        let status = server
            .get_app_status(Request::new(AppStatusRequest {
                job_id: deploy_resp.job_id,
                user_id: "user-1".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(status.host_id, "h1");
        assert!(!status.vm_id.is_empty());
    }

    #[tokio::test]
    async fn test_get_app_status_not_found_returns_not_found_error() {
        let server = make_server();
        let result = server
            .get_app_status(Request::new(AppStatusRequest {
                job_id: "nonexistent-job".to_string(),
                user_id: "user-1".to_string(),
            }))
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn test_get_app_status_wrong_user_returns_permission_denied() {
        let server = make_server();
        let deploy_resp = server
            .deploy_app(Request::new(deploy_req("user-1")))
            .await
            .unwrap()
            .into_inner();

        let result = server
            .get_app_status(Request::new(AppStatusRequest {
                job_id: deploy_resp.job_id,
                user_id: "user-wrong".to_string(),
            }))
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn test_cancel_app_success_and_status_becomes_cancelled() {
        let server = make_server();
        register_worker(&server, "h1").await;
        add_metrics(&server, "h1").await;

        let job_id = server
            .deploy_app(Request::new(deploy_req("user-1")))
            .await
            .unwrap()
            .into_inner()
            .job_id;

        let cancel_resp = server
            .cancel_app(Request::new(CancelRequest {
                job_id: job_id.clone(),
                user_id: "user-1".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(cancel_resp.success);

        let status = server
            .get_app_status(Request::new(AppStatusRequest {
                job_id,
                user_id: "user-1".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(status.status, crate::job::JobStatus::Cancelled as i32);
    }

    #[tokio::test]
    async fn test_cancel_nonexistent_job_returns_failure() {
        let server = make_server();
        let resp = server
            .cancel_app(Request::new(CancelRequest {
                job_id: "no-such-job".to_string(),
                user_id: "user-1".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!resp.success);
        assert!(!resp.message.is_empty());
    }

    #[tokio::test]
    async fn test_list_apps_returns_empty_initially() {
        let server = make_server();
        let resp = server
            .list_apps(Request::new(ListAppsRequest {
                status: None,
                user_id: "user-1".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.apps.is_empty());
    }

    #[tokio::test]
    async fn test_list_apps_filtered_by_user_id() {
        let server = make_server();
        server
            .deploy_app(Request::new(DeployRequest {
                app_id: "a1".to_string(),
                app_name: "app-one".to_string(),
                image: "nginx".to_string(),
                config: None,
                user_id: "user-1".to_string(),
            }))
            .await
            .unwrap();
        server
            .deploy_app(Request::new(DeployRequest {
                app_id: "a2".to_string(),
                app_name: "app-two".to_string(),
                image: "nginx".to_string(),
                config: None,
                user_id: "user-2".to_string(),
            }))
            .await
            .unwrap();

        let resp = server
            .list_apps(Request::new(ListAppsRequest {
                status: None,
                user_id: "user-1".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp.apps.len(), 1);
        assert_eq!(resp.apps[0].app_name, "app-one");
    }

    #[tokio::test]
    async fn test_list_apps_info_fields_are_populated() {
        let server = make_server();
        server
            .deploy_app(Request::new(DeployRequest {
                app_id: "app-xyz".to_string(),
                app_name: "my-service".to_string(),
                image: "redis:7".to_string(),
                config: None,
                user_id: "user-1".to_string(),
            }))
            .await
            .unwrap();

        let resp = server
            .list_apps(Request::new(ListAppsRequest {
                status: None,
                user_id: "user-1".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp.apps.len(), 1);
        let app = &resp.apps[0];
        assert_eq!(app.app_id, "app-xyz");
        assert_eq!(app.app_name, "my-service");
        assert_eq!(app.image, "redis:7");
        assert!(!app.job_id.is_empty());
    }

    #[tokio::test]
    async fn test_clone_shares_worker_registry() {
        let original = make_server();
        register_worker(&original, "h1").await;

        let cloned = original.clone();
        // Report metrics via clone — worker was registered on original
        let resp = cloned
            .report_metrics(Request::new(ReportMetricsRequest {
                host_id: "h1".to_string(),
                cpu_usage: 0.2,
                ram_used_bytes: GIB,
                ram_total_bytes: 4 * GIB,
                disk_used_bytes: 0,
                disk_total_bytes: 100 * GIB,
                apps_count: 0,
                timestamp: 0,
                load_avg_1: 0.1,
                load_avg_5: 0.1,
                load_avg_15: 0.1,
                vms: HashMap::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        // success=true means worker was found → registry is shared
        assert!(resp.success);
    }

    // ── forward_deploy_to_agent ──────────────────────────────────────────────

    fn default_vm_config() -> crate::job::VmConfig {
        crate::job::VmConfig {
            vcpus: 1,
            memory_mib: 256,
            disk_mib: 1024,
            env: Default::default(),
            ip_address: None,
            gateway: None,
            mac_address: None,
            volumes: vec![],
        }
    }

    #[tokio::test]
    async fn test_forward_deploy_to_agent_returns_not_found_for_unregistered_worker() {
        let server = make_server();
        let result = server
            .forward_deploy_to_agent(
                "nonexistent-host",
                "app-1",
                "nginx:latest",
                "vm-1",
                &default_vm_config(),
            )
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn test_forward_deploy_to_agent_returns_unavailable_when_agent_port_not_listening() {
        let server = make_server();
        // Register a worker pointing at a port that has nothing listening.
        server
            .register_worker(Request::new(RegisterWorkerRequest {
                host_id: "dead-host".to_string(),
                hostname: "dead-node".to_string(),
                ip_address: "127.0.0.1".to_string(),
                agent_port: 59980,
            }))
            .await
            .unwrap();

        let result = server
            .forward_deploy_to_agent(
                "dead-host",
                "app-1",
                "nginx:latest",
                "vm-1",
                &default_vm_config(),
            )
            .await;
        assert!(result.is_err());
        let code = result.unwrap_err().code();
        assert!(
            code == tonic::Code::Unavailable || code == tonic::Code::Internal,
            "expected Unavailable or Internal, got {:?}",
            code
        );
    }

    #[tokio::test]
    async fn test_deploy_app_returns_failed_when_forward_to_agent_fails() {
        let server = make_server();
        // Worker registered but with unreachable port — forward will fail silently.
        server
            .register_worker(Request::new(RegisterWorkerRequest {
                host_id: "unreachable".to_string(),
                hostname: "unreachable-node".to_string(),
                ip_address: "127.0.0.1".to_string(),
                agent_port: 59981,
            }))
            .await
            .unwrap();
        server
            .report_metrics(Request::new(ReportMetricsRequest {
                host_id: "unreachable".to_string(),
                cpu_usage: 0.1,
                ram_used_bytes: 512 * 1024 * 1024,
                ram_total_bytes: 4 * GIB,
                disk_used_bytes: 10 * GIB,
                disk_total_bytes: 100 * GIB,
                apps_count: 0,
                timestamp: 0,
                load_avg_1: 0.1,
                load_avg_5: 0.1,
                load_avg_15: 0.1,
                vms: HashMap::new(),
            }))
            .await
            .unwrap();

        let resp = server
            .deploy_app(Request::new(deploy_req("user-1")))
            .await
            .unwrap()
            .into_inner();

        // Agent at port 59981 is unreachable → job transitions to Failed.
        assert_eq!(resp.status, crate::job::JobStatus::Failed as i32);
        assert!(!resp.job_id.is_empty());
        assert_eq!(resp.host_id, "unreachable");
    }

    #[tokio::test]
    async fn test_stop_vm_on_agent_worker_not_found_returns_ok() {
        let server = make_server();
        // When the host_id does not exist in the registry, stop_vm_on_agent
        // logs a warning and returns Ok (best-effort, non-blocking).
        let result = server.stop_vm_on_agent("unknown-host", "any-vm").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_stop_vm_on_agent_agent_unreachable_returns_ok() {
        // Register a worker whose agent port is not listening.
        // stop_vm_on_agent should log a warning and return Ok (best-effort).
        let server = make_server();
        register_worker(&server, "h-unreachable").await;
        let result = server.stop_vm_on_agent("h-unreachable", "vm-xyz").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cancel_app_calls_stop_vm_on_agent_without_error() {
        // cancel_app ignores the result of stop_vm_on_agent; this test ensures
        // the cancel path completes successfully even when there is no live agent.
        let server = make_server();
        register_worker(&server, "h1").await;
        add_metrics(&server, "h1").await;
        let job_id = server
            .deploy_app(Request::new(deploy_req("user-1")))
            .await
            .unwrap()
            .into_inner()
            .job_id;

        let resp = server
            .cancel_app(Request::new(CancelRequest {
                job_id,
                user_id: "user-1".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.success);
    }

    #[tokio::test]
    async fn test_delete_app_success_removes_it_from_scheduler() {
        let server = make_server();
        let job_id = server
            .deploy_app(Request::new(deploy_req("user-1")))
            .await
            .unwrap()
            .into_inner()
            .job_id;

        // Verify it exists
        assert!(server.scheduler.get_job(&job_id).is_some());

        let delete_resp = server
            .delete_app(Request::new(DeleteAppRequest {
                job_id: job_id.clone(),
                user_id: "user-1".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(delete_resp.success);

        // Verify it's gone
        assert!(server.scheduler.get_job(&job_id).is_none());
    }

    #[tokio::test]
    async fn test_delete_nonexistent_job_returns_failure() {
        let server = make_server();
        let resp = server
            .delete_app(Request::new(DeleteAppRequest {
                job_id: "no-such-job".to_string(),
                user_id: "user-1".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!resp.success);
        assert!(!resp.message.is_empty());
    }

    #[tokio::test]
    async fn test_clone_shares_jobs() {
        let original = make_server();
        // Deploy a failed job (no workers) — creates a job in original
        let deploy_resp = original
            .deploy_app(Request::new(deploy_req("user-1")))
            .await
            .unwrap()
            .into_inner();

        let cloned = original.clone();
        // Clone shares the same jobs map — job from original MUST be visible
        let status = cloned
            .get_app_status(Request::new(AppStatusRequest {
                job_id: deploy_resp.job_id.clone(),
                user_id: "user-1".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(status.job_id, deploy_resp.job_id);
    }

    #[tokio::test]
    async fn test_deploy_app_assigns_networking_in_correct_range() {
        let server = make_server();
        // Register a worker first
        server
            .register_worker(Request::new(RegisterWorkerRequest {
                host_id: "h1".to_string(),
                hostname: "host1".to_string(),
                ip_address: "127.0.0.1".to_string(),
                agent_port: 5003,
            }))
            .await
            .unwrap();

        // Report metrics so it's available
        server
            .report_metrics(Request::new(ReportMetricsRequest {
                host_id: "h1".to_string(),
                cpu_usage: 0.1,
                ram_used_bytes: 0,
                ram_total_bytes: 4 * 1024 * 1024 * 1024,
                disk_used_bytes: 0,
                disk_total_bytes: 100 * 1024 * 1024 * 1024,
                apps_count: 0,
                timestamp: 0,
                load_avg_1: 0.0,
                load_avg_5: 0.0,
                load_avg_15: 0.0,
                vms: HashMap::new(),
            }))
            .await
            .unwrap();

        let resp = server
            .deploy_app(Request::new(deploy_req("user-1")))
            .await
            .unwrap()
            .into_inner();

        let job = server.scheduler.get_job(&resp.job_id).unwrap();

        let ip = job
            .config
            .ip_address
            .as_ref()
            .expect("IP should be assigned");
        let gw = job
            .config
            .gateway
            .as_ref()
            .expect("Gateway should be assigned");
        let mac = job
            .config
            .mac_address
            .as_ref()
            .expect("MAC should be assigned");

        assert!(
            ip.starts_with("172.16.1."),
            "IP {} should be in 172.16.1.x range",
            ip
        );
        assert_eq!(gw, "172.16.1.1");
        assert!(
            mac.starts_with("AA:BB:CC:01:01:"),
            "MAC {} should have correct prefix",
            mac
        );
    }

    #[tokio::test]
    async fn test_deploy_app_maps_volumes_correctly() {
        let server = make_server();
        server
            .register_worker(Request::new(RegisterWorkerRequest {
                host_id: "h1".to_string(),
                hostname: "host1".to_string(),
                ip_address: "127.0.0.1".to_string(),
                agent_port: 5003,
            }))
            .await
            .unwrap();

        server
            .report_metrics(Request::new(ReportMetricsRequest {
                host_id: "h1".to_string(),
                cpu_usage: 0.1,
                ram_used_bytes: 0,
                ram_total_bytes: 4 * 1024 * 1024 * 1024,
                disk_used_bytes: 0,
                disk_total_bytes: 100 * 1024 * 1024 * 1024,
                apps_count: 0,
                timestamp: 0,
                load_avg_1: 0.0,
                load_avg_5: 0.0,
                load_avg_15: 0.0,
                vms: HashMap::new(),
            }))
            .await
            .unwrap();

        let req = DeployRequest {
            app_id: "app-1".to_string(),
            app_name: "test-app".to_string(),
            image: "alpine".to_string(),
            user_id: "user-1".to_string(),
            config: Some(AppConfig {
                vcpus: 1,
                memory_mib: 128,
                disk_mib: 512,
                env: HashMap::new(),
                ip_address: String::new(),
                gateway: String::new(),
                mac_address: String::new(),
                volumes: vec![mikrom_proto::scheduler::Volume {
                    volume_id: "data-vol".to_string(),
                    size_mib: 500,
                    read_only: true,
                }],
            }),
        };

        let resp = server
            .deploy_app(Request::new(req))
            .await
            .unwrap()
            .into_inner();
        let job = server.scheduler.get_job(&resp.job_id).unwrap();

        assert_eq!(job.config.volumes.len(), 1);
        assert_eq!(job.config.volumes[0].volume_id, "data-vol");
        assert_eq!(job.config.volumes[0].size_mib, 500);
        assert!(job.config.volumes[0].read_only);
    }

    #[tokio::test]
    async fn test_pause_app_returns_not_found_for_invalid_job() {
        let server = make_server();
        let resp = server
            .pause_app(Request::new(PauseRequest {
                job_id: "invalid".to_string(),
                user_id: "user-1".to_string(),
            }))
            .await;

        assert!(resp.is_err());
        assert_eq!(resp.unwrap_err().code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn test_resume_app_returns_not_found_for_invalid_job() {
        let server = make_server();
        let resp = server
            .resume_app(Request::new(ResumeRequest {
                job_id: "invalid".to_string(),
                user_id: "user-1".to_string(),
            }))
            .await;

        assert!(resp.is_err());
        assert_eq!(resp.unwrap_err().code(), tonic::Code::NotFound);
    }
}
