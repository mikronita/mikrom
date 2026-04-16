use crate::metrics::HostMetrics;
use crate::scheduler::AppScheduler;
use crate::worker_registry::WorkerRegistry;
use mikrom_proto::agent::{
    StartVmRequest, StartVmResponse, agent_service_client::AgentServiceClient,
};
use mikrom_proto::scheduler::{
    AppInfo, AppStatusRequest, AppStatusResponse, CancelRequest, CancelResponse, DeployRequest,
    DeployResponse, ListAppsRequest, ListAppsResponse, RegisterWorkerRequest,
    RegisterWorkerResponse, ReportMetricsRequest, ReportMetricsResponse,
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

        tracing::info!(
            "Registering worker: {} ({}) on {}:{}",
            req.hostname,
            req.host_id,
            req.ip_address,
            req.agent_port
        );

        let success = self.scheduler.worker_registry().register(
            req.host_id,
            req.hostname,
            req.ip_address,
            req.agent_port as u16,
        );

        Ok(Response::new(RegisterWorkerResponse {
            success,
            message: if success {
                "Registered".to_string()
            } else {
                "Failed".to_string()
            },
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
            env: req
                .config
                .as_ref()
                .map(|c| c.env.clone())
                .unwrap_or_default(),
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

                let _ = self
                    .forward_deploy_to_agent(&host_id, &app_id, &image, &vm_id, &config)
                    .await;

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
                let _ = self
                    .stop_vm_on_agent(job.host_id.as_deref().unwrap_or(""), vm_id)
                    .await;
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

    async fn forward_deploy_to_agent(
        &self,
        host_id: &str,
        app_id: &str,
        image: &str,
        vm_id: &str,
        config: &crate::job::VmConfig,
    ) -> Result<(), Status> {
        let worker = self
            .scheduler
            .worker_registry()
            .get_worker(host_id)
            .ok_or_else(|| Status::not_found("Worker not found"))?;

        // With mTLS: connect by hostname (must match the SAN in the agent's cert).
        // Without TLS: connect by IP as before.
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

        let channel = endpoint
            .connect()
            .await
            .map_err(|e| Status::unavailable(format!("Failed to connect to agent: {}", e)))?;
        let mut client = AgentServiceClient::new(channel);

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

        let resp = client
            .start_vm(req)
            .await
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
            certs: self.certs.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mikrom_proto::scheduler::{
        AppConfig, AppStatusRequest, CancelRequest, DeployRequest, ListAppsRequest,
        RegisterWorkerRequest, ReportMetricsRequest,
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
    async fn test_deploy_app_with_available_worker_returns_scheduled_status() {
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
                }),
                user_id: "user-1".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp.status, crate::job::JobStatus::Scheduled as i32);
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
        // Default config (vcpus=1, memory_mib=256, disk_mib=1024) fits the worker
        assert_eq!(resp.status, crate::job::JobStatus::Scheduled as i32);
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
            }))
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::NotFound);
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
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(cancel_resp.success);

        let status = server
            .get_app_status(Request::new(AppStatusRequest { job_id }))
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
            }))
            .await
            .unwrap()
            .into_inner();
        // success=true means worker was found → registry is shared
        assert!(resp.success);
    }

    #[tokio::test]
    async fn test_clone_does_not_share_jobs() {
        let original = make_server();
        // Deploy a failed job (no workers) — creates a job in original
        let deploy_resp = original
            .deploy_app(Request::new(deploy_req("user-1")))
            .await
            .unwrap()
            .into_inner();

        let cloned = original.clone();
        // Clone has a fresh jobs map — job from original is not visible
        let result = cloned
            .get_app_status(Request::new(AppStatusRequest {
                job_id: deploy_resp.job_id,
            }))
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::NotFound);
    }
}
