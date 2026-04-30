use crate::scheduler::AppScheduler;
use crate::worker_registry::WorkerRegistry;
use mikrom_proto::scheduler::{
    AppStatusResponse, DeployResponse, GetLogsRequest, GetLogsResponse, ListWorkersRequest,
    ListWorkersResponse, RegisterWorkerRequest, RegisterWorkerResponse, ReportMetricsRequest,
    ReportMetricsResponse, WatchAppsRequest, WatchAppsResponse,
};

use mikrom_proto::tls::ServiceCerts;
use sqlx::PgPool;
use std::net::SocketAddr;
use tonic::{Response, Status};
use uuid::Uuid;

pub struct SchedulerServer {
    scheduler: AppScheduler,
    nats_client: async_nats::Client,
    certs: Option<ServiceCerts>,
}

impl SchedulerServer {
    pub fn new(
        pool: PgPool,
        nats_client: async_nats::Client,
        certs: Option<ServiceCerts>,
    ) -> anyhow::Result<Self> {
        let worker_registry = WorkerRegistry::new(pool.clone());
        let mut scheduler = AppScheduler::new(pool, worker_registry);
        scheduler.set_nats_client(nats_client.clone());

        Ok(Self {
            scheduler,
            nats_client,
            certs,
        })
    }

    #[must_use]
    pub fn scheduler(&self) -> &AppScheduler {
        &self.scheduler
    }
}

#[tonic::async_trait]
impl mikrom_proto::scheduler::scheduler_service_server::SchedulerService for SchedulerServer {
    #[tracing::instrument(skip(self, request), fields(app_id = %request.get_ref().app_id))]
    async fn deploy_app(
        &self,
        request: tonic::Request<mikrom_proto::scheduler::DeployRequest>,
    ) -> Result<Response<DeployResponse>, Status> {
        let req = request.into_inner();
        let job_id = Uuid::new_v4().to_string();
        let vm_id = Uuid::new_v4().to_string();

        let config = match req.config {
            Some(c) => crate::job::VmConfig {
                vcpus: c.vcpus,
                memory_mib: u64::from(c.memory_mib),
                disk_mib: u64::from(c.disk_mib),
                port: c.port,
                env: c.env,
                ip_address: None,
                gateway: None,
                mac_address: None,
                netmask: None,
                volumes: c
                    .volumes
                    .iter()
                    .map(|v| crate::job::Volume {
                        volume_id: v.volume_id.clone(),
                        size_mib: v.size_mib,
                        read_only: v.read_only,
                    })
                    .collect(),
            },
            None => crate::job::VmConfig::default(),
        };

        let result = self
            .scheduler
            .select_best_worker(&config, &req.app_id)
            .await;

        match result {
            Ok(worker) => {
                let app_id_real = req.app_id.clone();
                let image_real = req.image.clone();
                let host_id = worker.host_id.clone();
                tracing::info!(host_id = %host_id, "Selected worker, allocating IP...");

                // Allocate IP from worker's IPAM
                let allocation = match worker.ipam.allocate().await {
                    Ok(a) => a,
                    Err(e) => {
                        tracing::error!(host_id = %host_id, error = %e, "Failed to allocate IP");
                        return Ok(Response::new(DeployResponse {
                            message: format!("IPAM error: {}", e),
                            ..Default::default()
                        }));
                    },
                };

                let netmask = worker.ipam.netmask();
                let (ip_address, gateway, mac_address) = if let Some(a) = allocation {
                    tracing::info!(ip = %a.ip, "Allocated IP successfully");
                    (Some(a.ip), Some(a.gateway), Some(a.mac))
                } else {
                    tracing::warn!("No IP allocated (pool exhausted?)");
                    return Ok(Response::new(DeployResponse {
                        message: "IP address pool exhausted".to_string(),
                        ..Default::default()
                    }));
                };

                let job_config = crate::job::VmConfig {
                    vcpus: config.vcpus,
                    memory_mib: config.memory_mib,
                    disk_mib: config.disk_mib,
                    port: config.port,
                    env: config.env.clone(),
                    ip_address,
                    gateway,
                    mac_address,
                    netmask: Some(netmask),
                    volumes: config.volumes.clone(),
                };

                let mut job = crate::job::Job::new(
                    job_id.clone(),
                    app_id_real.clone(),
                    req.app_name.clone(),
                    image_real.clone(),
                    job_config.clone(),
                    req.user_id.clone(),
                    Some(req.deployment_id),
                );

                job.schedule(host_id.clone(), vm_id.clone());

                // ── Exclusivity Cluster-wide ──────────────────────────────────
                tracing::info!("Checking for existing instances to pause...");
                let other_jobs: Vec<String> = match self.scheduler.list_jobs(None, None).await {
                    Ok(jobs) => jobs
                        .into_iter()
                        .filter(|j| {
                            j.app_id == req.app_id
                                && j.job_id != job_id
                                && j.status != crate::job::JobStatus::Stopped
                                && j.status != crate::job::JobStatus::Cancelled
                                && j.status != crate::job::JobStatus::Failed
                        })
                        .map(|j| j.job_id)
                        .collect(),
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to list jobs for exclusivity check");
                        return Ok(Response::new(DeployResponse {
                            message: format!("Database error listing jobs: {}", e),
                            ..Default::default()
                        }));
                    },
                };

                for old_job_id in other_jobs {
                    tracing::info!(new_job_id = %job_id, old_job_id = %old_job_id, app_id = %req.app_id, "Pausing existing cluster instance for exclusivity");
                    let _ = self.pause_job_internal(&old_job_id, &req.user_id).await;
                }

                if let Err(e) = self.scheduler.add_job(job).await {
                    tracing::error!(error = %e, "Failed to add job to database");
                    return Ok(Response::new(DeployResponse {
                        message: format!("Database error adding job: {}", e),
                        ..Default::default()
                    }));
                }

                tracing::info!(
                    job_id = %job_id,
                    host_id = %host_id,
                    "Job persisted, forwarding to agent..."
                );

                // ── Agent Forwarding ──────────────────────────────────────────
                if let Err(e) = self
                    .forward_deploy_to_agent(
                        &host_id,
                        &app_id_real,
                        &image_real,
                        &vm_id,
                        &job_config,
                    )
                    .await
                {
                    tracing::error!(job_id = %job_id, error = %e, "Failed to deploy to agent");
                    self.scheduler.fail_job(&job_id, e.to_string()).await.ok();
                    return Ok(Response::new(DeployResponse {
                        message: format!("Agent error: {}", e.message()),
                        ..Default::default()
                    }));
                }

                // Mark as running
                self.scheduler.start_job(&job_id).await.ok();

                Ok(Response::new(DeployResponse {
                    job_id,
                    status: mikrom_proto::scheduler::DeployStatus::Running as i32,
                    host_id,
                    vm_id,
                    message: "Deployment successful".to_string(),
                    ip_address: job_config.ip_address.unwrap_or_default(),
                }))
            },
            Err(e) => {
                tracing::error!(error = %e, "No viable workers found for deployment");
                Ok(Response::new(DeployResponse {
                    message: format!("No viable workers found: {}", e),
                    ..Default::default()
                }))
            },
        }
    }

    async fn get_app_status(
        &self,
        request: tonic::Request<mikrom_proto::scheduler::AppStatusRequest>,
    ) -> Result<Response<AppStatusResponse>, Status> {
        let req = request.into_inner();
        let job = self
            .scheduler
            .get_job(&req.job_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        match job {
            Some(j) => {
                if j.user_id != req.user_id && req.user_id != "system" {
                    return Err(Status::permission_denied("Unauthorized"));
                }

                let (cpu_usage, ram_used_bytes) = if let Some(host_id) = &j.host_id {
                    if let Some(metrics) = self
                        .scheduler
                        .worker_registry()
                        .get_metrics(host_id)
                        .await
                        .map_err(|e| Status::internal(e.to_string()))?
                    {
                        if let Some(vm_id) = &j.vm_id {
                            if let Some(vm_m) = metrics.vms.get(vm_id) {
                                (vm_m.cpu_usage, vm_m.ram_used_bytes)
                            } else {
                                (0.0, 0)
                            }
                        } else {
                            (0.0, 0)
                        }
                    } else {
                        (0.0, 0)
                    }
                } else {
                    (0.0, 0)
                };

                let response = AppStatusResponse {
                    job_id: j.job_id,
                    status: j.status as i32,
                    host_id: j.host_id.unwrap_or_default(),
                    vm_id: j.vm_id.unwrap_or_default(),
                    scheduled_at: j.scheduled_at.unwrap_or(0),
                    started_at: j.started_at.unwrap_or(0),
                    stopped_at: j.stopped_at.unwrap_or(0),
                    error_message: j.error_message.unwrap_or_default(),
                    cpu_usage,
                    ram_used_bytes,
                    ip_address: j.config.ip_address.unwrap_or_default(),
                };
                Ok(Response::new(response))
            },
            None => Err(Status::not_found("Job not found")),
        }
    }

    async fn list_apps(
        &self,
        request: tonic::Request<mikrom_proto::scheduler::ListAppsRequest>,
    ) -> Result<Response<mikrom_proto::scheduler::ListAppsResponse>, Status> {
        let req = request.into_inner();
        let jobs = self
            .scheduler
            .list_jobs(Some(&req.user_id), None)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let mut apps = Vec::new();
        for j in jobs {
            let (cpu_usage, ram_used_bytes) = if let Some(host_id) = &j.host_id {
                if let Some(metrics) = self
                    .scheduler
                    .worker_registry()
                    .get_metrics(host_id)
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?
                {
                    if let Some(vm_id) = &j.vm_id {
                        if let Some(vm_m) = metrics.vms.get(vm_id) {
                            (vm_m.cpu_usage, vm_m.ram_used_bytes)
                        } else {
                            (0.0, 0)
                        }
                    } else {
                        (0.0, 0)
                    }
                } else {
                    (0.0, 0)
                }
            } else {
                (0.0, 0)
            };

            apps.push(mikrom_proto::scheduler::AppInfo {
                job_id: j.job_id,
                app_id: j.app_id,
                app_name: j.app_name,
                image: j.image,
                status: j.status as i32,
                host_id: j.host_id.unwrap_or_default(),
                vm_id: j.vm_id.unwrap_or_default(),
                cpu_usage,
                ram_used_bytes,
                user_id: j.user_id,
                deployment_id: j.deployment_id.unwrap_or_default(),
            });
        }

        Ok(Response::new(mikrom_proto::scheduler::ListAppsResponse {
            apps,
        }))
    }

    async fn pause_app(
        &self,
        request: tonic::Request<mikrom_proto::scheduler::PauseRequest>,
    ) -> Result<Response<mikrom_proto::scheduler::PauseResponse>, Status> {
        let req = request.into_inner();
        let (success, message) = self.pause_job_internal(&req.job_id, &req.user_id).await?;

        Ok(Response::new(mikrom_proto::scheduler::PauseResponse {
            success,
            message,
        }))
    }

    async fn resume_app(
        &self,
        request: tonic::Request<mikrom_proto::scheduler::ResumeRequest>,
    ) -> Result<Response<mikrom_proto::scheduler::ResumeResponse>, Status> {
        let req = request.into_inner();
        let job = self
            .scheduler
            .get_job(&req.job_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("Job not found"))?;

        if job.user_id != req.user_id {
            return Err(Status::permission_denied("Unauthorized"));
        }

        // ── Exclusivity Cluster-wide ──────────────────────────────────
        let app_id = job.app_id.clone();
        let other_jobs: Vec<String> = match self.scheduler.list_jobs(None, None).await {
            Ok(jobs) => jobs
                .into_iter()
                .filter(|j| {
                    j.app_id == app_id
                        && j.job_id != req.job_id
                        && j.status != crate::job::JobStatus::Stopped
                        && j.status != crate::job::JobStatus::Cancelled
                        && j.status != crate::job::JobStatus::Failed
                })
                .map(|j| j.job_id)
                .collect(),
            Err(e) => {
                tracing::error!(error = %e, "Failed to list jobs for exclusivity check during resume");
                return Err(Status::internal(e.to_string()));
            },
        };

        for old_job_id in other_jobs {
            tracing::info!(resume_job_id = %req.job_id, old_job_id = %old_job_id, "Pausing existing instance during resume");
            let _ = self.pause_job_internal(&old_job_id, &req.user_id).await;
        }

        let host_id = job.host_id.as_deref().unwrap_or("");
        let vm_id = job.vm_id.as_deref().unwrap_or("");

        if host_id.is_empty() || vm_id.is_empty() {
            return Err(Status::failed_precondition("Job not scheduled"));
        }

        let subject = format!("mikrom.agent.{}.cmd", host_id);

        use mikrom_proto::agent::{AgentCommand, AgentCommandResponse, ResumeVmRequest};
        use prost::Message;

        let resume_cmd = AgentCommand {
            command: Some(mikrom_proto::agent::agent_command::Command::ResumeVm(
                ResumeVmRequest {
                    vm_id: vm_id.to_string(),
                },
            )),
        };

        let mut payload = Vec::new();
        let _ = resume_cmd.encode(&mut payload);

        let (success, message) = match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.nats_client.request(subject, payload.into()),
        )
        .await
        {
            Ok(Ok(resp)) => match AgentCommandResponse::decode(&resp.payload[..]) {
                Ok(inner) => (inner.success, inner.message),
                Err(e) => (false, format!("Failed to decode agent response: {}", e)),
            },
            Ok(Err(e)) => (false, e.to_string()),
            Err(_) => (false, "Agent request timed out".to_string()),
        };

        if success {
            self.scheduler
                .update_job_status(&req.job_id, crate::job::JobStatus::Running)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
        }

        Ok(Response::new(mikrom_proto::scheduler::ResumeResponse {
            success,
            message,
        }))
    }

    async fn cancel_app(
        &self,
        request: tonic::Request<mikrom_proto::scheduler::CancelRequest>,
    ) -> Result<Response<mikrom_proto::scheduler::CancelResponse>, Status> {
        let req = request.into_inner();
        self.scheduler
            .cancel_job(&req.job_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(mikrom_proto::scheduler::CancelResponse {
            success: true,
            message: "Cancelled".to_string(),
        }))
    }

    async fn delete_app(
        &self,
        request: tonic::Request<mikrom_proto::scheduler::DeleteAppRequest>,
    ) -> Result<Response<mikrom_proto::scheduler::DeleteAppResponse>, Status> {
        let req = request.into_inner();
        let job = self
            .scheduler
            .get_job(&req.job_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("Job not found"))?;

        if job.user_id != req.user_id {
            return Err(Status::permission_denied("Unauthorized"));
        }

        let host_id = job.host_id.as_deref().unwrap_or("");
        let vm_id = job.vm_id.as_deref().unwrap_or("");

        if !host_id.is_empty() && !vm_id.is_empty() {
            let _ = self.delete_vm_on_agent(host_id, vm_id).await;
        }

        self.scheduler
            .remove_job(&req.job_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(mikrom_proto::scheduler::DeleteAppResponse {
            success: true,
            message: "Deleted".to_string(),
        }))
    }

    async fn register_worker(
        &self,
        _request: tonic::Request<RegisterWorkerRequest>,
    ) -> Result<Response<RegisterWorkerResponse>, Status> {
        Err(Status::unimplemented("Use NATS for worker registration"))
    }

    async fn report_metrics(
        &self,
        _request: tonic::Request<ReportMetricsRequest>,
    ) -> Result<Response<ReportMetricsResponse>, Status> {
        Err(Status::unimplemented("Use NATS for metrics reporting"))
    }

    async fn list_workers(
        &self,
        _request: tonic::Request<ListWorkersRequest>,
    ) -> Result<Response<ListWorkersResponse>, Status> {
        let workers = self
            .scheduler
            .worker_registry()
            .get_available_workers()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let worker_infos = workers
            .into_iter()
            .map(|w| mikrom_proto::scheduler::WorkerInfo {
                host_id: w.host_id,
                hostname: w.hostname,
                ip_address: w.ip_address,
                agent_port: w.agent_port as u32,
                bridge_ip: w.bridge_ip,
                last_heartbeat: w.last_heartbeat,
            })
            .collect();

        Ok(Response::new(ListWorkersResponse {
            workers: worker_infos,
        }))
    }

    type WatchAppsStream =
        tokio_stream::wrappers::ReceiverStream<Result<WatchAppsResponse, Status>>;
    async fn watch_apps(
        &self,
        _request: tonic::Request<WatchAppsRequest>,
    ) -> Result<Response<Self::WatchAppsStream>, Status> {
        Err(Status::unimplemented("Use NATS for app updates"))
    }

    type GetAppLogsStream = tokio_stream::wrappers::ReceiverStream<Result<GetLogsResponse, Status>>;
    async fn get_app_logs(
        &self,
        _request: tonic::Request<GetLogsRequest>,
    ) -> Result<Response<Self::GetAppLogsStream>, Status> {
        Err(Status::unimplemented("Use NATS for logs"))
    }
}

impl SchedulerServer {
    async fn forward_deploy_to_agent(
        &self,
        host_id: &str,
        app_id_real: &str,
        image: &str,
        vm_id: &str,
        config: &crate::job::VmConfig,
    ) -> Result<(), Status> {
        let subject = format!("mikrom.agent.{}.cmd", host_id);
        tracing::info!(host_id = %host_id, subject = %subject, "Attempting to send StartVm command via NATS");

        use mikrom_proto::agent::{AgentCommand, AgentCommandResponse, StartVmRequest, VmConfig};
        use prost::Message;

        let cmd = AgentCommand {
            command: Some(mikrom_proto::agent::agent_command::Command::StartVm(
                StartVmRequest {
                    vm_id: vm_id.to_string(),
                    app_id: app_id_real.to_string(),
                    image: image.to_string(),
                    config: Some(VmConfig {
                        vcpus: config.vcpus,
                        memory_mib: config.memory_mib as u32,
                        disk_mib: config.disk_mib as u32,
                        port: config.port,
                        env: config.env.clone(),
                        ip_address: config.ip_address.clone().unwrap_or_default(),
                        gateway: config.gateway.clone().unwrap_or_default(),
                        mac_address: config.mac_address.clone().unwrap_or_default(),
                        netmask: config.netmask.clone().unwrap_or_default(),
                        volumes: vec![],
                    }),
                },
            )),
        };

        let mut payload = Vec::new();
        cmd.encode(&mut payload).map_err(|e| {
            tracing::error!("Failed to encode AgentCommand: {}", e);
            Status::internal(e.to_string())
        })?;

        tracing::debug!("Encoded payload size: {} bytes", payload.len());

        match tokio::time::timeout(
            std::time::Duration::from_secs(15),
            self.nats_client.request(subject.clone(), payload.into()),
        )
        .await
        {
            Ok(Ok(response)) => {
                tracing::info!(host_id = %host_id, "Received response from agent");
                match AgentCommandResponse::decode(&response.payload[..]) {
                    Ok(inner) => {
                        if inner.success {
                            tracing::info!(host_id = %host_id, "Agent successfully started VM: {}", inner.message);
                            Ok(())
                        } else {
                            tracing::error!(host_id = %host_id, error = %inner.message, "Agent failed to start VM");
                            Err(Status::internal(inner.message))
                        }
                    },
                    Err(e) => {
                        tracing::error!(host_id = %host_id, error = %e, "Failed to decode agent response");
                        Err(Status::internal(format!(
                            "Failed to decode agent response: {}",
                            e
                        )))
                    },
                }
            },
            Ok(Err(e)) => {
                tracing::error!(host_id = %host_id, error = %e, "NATS request failed");
                Err(Status::internal(format!("NATS command failed: {}", e)))
            },
            Err(_) => {
                tracing::error!(host_id = %host_id, subject = %subject, "Timeout waiting for agent response (15s)");
                Err(Status::deadline_exceeded(
                    "Timeout waiting for agent response",
                ))
            },
        }
    }

    async fn pause_job_internal(
        &self,
        job_id: &str,
        user_id: &str,
    ) -> Result<(bool, String), Status> {
        let job = self
            .scheduler
            .get_job(job_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| {
                tracing::warn!("Job {} not found", job_id);
                Status::not_found("Job not found")
            })?;

        if job.user_id != user_id {
            tracing::warn!("User {} unauthorized for job {}", user_id, job_id);
            return Err(Status::permission_denied("You do not own this job"));
        }

        // If it's already stopped, just return success
        if job.status == crate::job::JobStatus::Stopped {
            return Ok((true, "Already stopped".to_string()));
        }

        let host_id = job.host_id.as_deref().unwrap_or("");
        let vm_id = job.vm_id.as_deref().unwrap_or("");

        if host_id.is_empty() || vm_id.is_empty() {
            return Ok((true, "Job not scheduled".to_string()));
        }

        let subject = format!("mikrom.agent.{}.cmd", host_id);

        use mikrom_proto::agent::{
            AgentCommand, AgentCommandResponse, PauseVmRequest, StopVmRequest,
        };
        use prost::Message;

        let pause_cmd = AgentCommand {
            command: Some(mikrom_proto::agent::agent_command::Command::PauseVm(
                PauseVmRequest {
                    vm_id: vm_id.to_string(),
                },
            )),
        };

        let mut payload = Vec::new();
        pause_cmd
            .encode(&mut payload)
            .map_err(|e| Status::internal(e.to_string()))?;

        let (mut success, mut message) = match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.nats_client.request(subject.clone(), payload.into()),
        )
        .await
        {
            Ok(Ok(resp)) => match AgentCommandResponse::decode(&resp.payload[..]) {
                Ok(inner) => (inner.success, inner.message),
                Err(e) => (false, format!("Failed to decode agent response: {}", e)),
            },
            Ok(Err(e)) => (false, e.to_string()),
            Err(_) => (false, "Agent request timed out".to_string()),
        };

        if !success {
            // FALLBACK: Forced stop if hibernation fails
            tracing::warn!(
                "Agent reported pause failure for {}, forcing stop via NATS",
                job_id
            );
            let stop_cmd = AgentCommand {
                command: Some(mikrom_proto::agent::agent_command::Command::StopVm(
                    StopVmRequest {
                        vm_id: vm_id.to_string(),
                    },
                )),
            };
            let mut stop_payload = Vec::new();
            let _ = stop_cmd.encode(&mut stop_payload);

            match tokio::time::timeout(
                std::time::Duration::from_secs(5),
                self.nats_client.request(subject, stop_payload.into()),
            )
            .await
            {
                Ok(Ok(resp)) => match AgentCommandResponse::decode(&resp.payload[..]) {
                    Ok(inner) => {
                        if inner.success {
                            success = true;
                            message =
                                format!("Pause failed but fallback stop succeeded: {}", message);
                        } else {
                            message =
                                format!("Pause failed and fallback stop failed: {}", inner.message);
                        }
                    },
                    Err(e) => {
                        message = format!("Pause failed and fallback decode failed: {}", e);
                    },
                },
                Ok(Err(e)) => {
                    message = format!("Pause failed and fallback NATS request failed: {}", e);
                },
                Err(_) => {
                    message = "Pause failed and fallback request timed out".to_string();
                },
            }
        }

        if success {
            self.scheduler
                .update_job_status(job_id, crate::job::JobStatus::Stopped)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
        }

        Ok((success, message))
    }

    async fn delete_vm_on_agent(&self, host_id: &str, vm_id: &str) -> Result<(), Status> {
        if host_id.is_empty() {
            return Ok(());
        }

        let subject = format!("mikrom.agent.{}.cmd", host_id);

        use mikrom_proto::agent::{AgentCommand, AgentCommandResponse, DeleteVmRequest};
        use prost::Message;

        let cmd = AgentCommand {
            command: Some(mikrom_proto::agent::agent_command::Command::DeleteVm(
                DeleteVmRequest {
                    vm_id: vm_id.to_string(),
                },
            )),
        };
        let mut payload = Vec::new();
        let _ = cmd.encode(&mut payload);

        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.nats_client.request(subject, payload.into()),
        )
        .await
        {
            Ok(Ok(resp)) => match AgentCommandResponse::decode(&resp.payload[..]) {
                Ok(inner) => {
                    if inner.success {
                        tracing::info!("VM {} resources purged on host {}", vm_id, host_id);
                    } else {
                        tracing::warn!("Agent failed to purge VM {}: {}", vm_id, inner.message);
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to decode agent delete response: {}", e);
                },
            },
            Ok(Err(e)) => {
                tracing::warn!(
                    "Failed to send delete command to agent {} for VM {}: {}",
                    host_id,
                    vm_id,
                    e
                );
            },
            Err(_) => {
                tracing::warn!(
                    "Delete command to agent {} for VM {} timed out",
                    host_id,
                    vm_id
                );
            },
        }

        Ok(())
    }

    pub async fn serve(&self, addr: SocketAddr) -> anyhow::Result<()> {
        use mikrom_proto::scheduler::scheduler_service_server::SchedulerServiceServer;
        use tonic::transport::Server;

        let mut builder = Server::builder();

        if let Some(certs) = &self.certs {
            builder = builder.tls_config(certs.server_tls_config()?)?;
        }

        builder
            .add_service(SchedulerServiceServer::new(self.clone()))
            .serve(addr)
            .await
            .map_err(Into::into)
    }
}

impl Clone for SchedulerServer {
    fn clone(&self) -> Self {
        Self {
            scheduler: self.scheduler.clone(),
            nats_client: self.nats_client.clone(),
            certs: self.certs.clone(),
        }
    }
}
