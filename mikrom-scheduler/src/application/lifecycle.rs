use crate::application::{AppContext, publish_job_update_best_effort};
use crate::domain::{DomainError, DomainResult, Job, JobStatus};
use std::sync::Arc;

#[derive(Clone)]
pub struct JobLifecycleService {
    ctx: Arc<AppContext>,
}

impl JobLifecycleService {
    pub fn new(ctx: Arc<AppContext>) -> Self {
        Self { ctx }
    }

    pub async fn get_app_status(&self, job_id: &str, tenant_id: &str) -> DomainResult<Job> {
        let telemetry = self.ctx.telemetry.clone();
        telemetry
            .observe_result("lifecycle", "get_app_status", async {
                let job = self
                    .ctx
                    .job_repo
                    .get_job(job_id)
                    .await?
                    .ok_or_else(|| DomainError::JobNotFound(job_id.to_string()))?;

                if job.tenant_id != tenant_id.into() && tenant_id != "system" {
                    return Err(DomainError::Unauthorized(
                        "You do not own this job".to_string(),
                    ));
                }

                Ok(job)
            })
            .await
    }

    pub async fn pause_app(&self, job_id: &str, tenant_id: &str) -> DomainResult<()> {
        let telemetry = self.ctx.telemetry.clone();
        telemetry
            .observe_result("lifecycle", "pause_app", async {
                let job = self.get_app_status(job_id, tenant_id).await?;

                if matches!(job.status, JobStatus::Paused | JobStatus::Stopped) {
                    tracing::info!(
                        job_id = %job_id,
                        status = %job.status.as_str(),
                        "Pause requested for a job that is already paused or stopped"
                    );
                    return Ok(());
                }

                if tenant_id != "system"
                    && let Ok(Some(app)) = self.ctx.app_repo.get_app_config(&job.app_id).await
                {
                    let now = chrono::Utc::now().timestamp();
                    if app.last_router_traffic_at > app.last_scaled_to_zero_at
                        && now - app.last_router_traffic_at
                            < self.ctx.runtime.router_idle_timeout_secs
                    {
                        tracing::info!(
                            job_id = %job_id,
                            app_id = %job.app_id,
                            "Aborting hibernation: traffic detected just before pause"
                        );
                        return Ok(());
                    }
                }

                tracing::info!(
                    job_id = %job_id,
                    app_id = %job.app_id,
                    vm_id = ?job.vm_id,
                    "Pausing job"
                );

                if let (Some(host_id), Some(vm_id)) = (&job.host_id, &job.vm_id) {
                    if let Err(e) = self.ctx.agent_client.pause_vm(host_id, vm_id).await {
                        tracing::warn!(
                            "Pause failed for {}, attempting stop fallback: {}",
                            job_id,
                            e
                        );
                        self.ctx.agent_client.stop_vm(host_id, vm_id).await?;
                    }
                    self.ctx
                        .job_repo
                        .update_job_status(job_id, JobStatus::Paused)
                        .await?;

                    let mut updated_job = job;
                    updated_job.status = JobStatus::Paused;
                    publish_job_update_best_effort(
                        self.ctx.nats_client.as_ref(),
                        &updated_job,
                        "pause-app-job-update",
                    )
                    .await;
                }

                tracing::info!(job_id = %job_id, "Job paused successfully");
                Ok(())
            })
            .await
    }

    pub async fn resume_app(&self, job_id: &str, tenant_id: &str) -> DomainResult<bool> {
        let telemetry = self.ctx.telemetry.clone();
        telemetry
            .observe_result("lifecycle", "resume_app", async {
                let job = self.get_app_status(job_id, tenant_id).await?;

                let Some(host_id) = job.host_id.as_ref() else {
                    return Ok(false);
                };
                let Some(vm_id) = job.vm_id.as_ref() else {
                    return Ok(false);
                };

                if self.ctx.worker_repo.get_worker(host_id).await?.is_none() {
                    let message = format!("Host {host_id} no longer exists");
                    tracing::warn!(
                        job_id = %job_id,
                        app_id = %job.app_id,
                        host_id = %host_id,
                        "Invalidating paused job linked to a missing host"
                    );

                    self.ctx
                        .job_repo
                        .fail_job(job_id, message.clone(), chrono::Utc::now().timestamp())
                        .await?;

                    let mut updated_job = job;
                    updated_job.status = JobStatus::Failed;
                    updated_job.stopped_at = Some(chrono::Utc::now().timestamp());
                    updated_job.error_message = Some(message);
                    publish_job_update_best_effort(
                        self.ctx.nats_client.as_ref(),
                        &updated_job,
                        "resume-app-host-missing",
                    )
                    .await;

                    return Ok(false);
                }

                tracing::info!(
                    job_id = %job_id,
                    app_id = %job.app_id,
                    vm_id = ?job.vm_id,
                    "Resuming job"
                );

                self.ctx.agent_client.resume_vm(host_id, vm_id).await?;
                if let Err(e) = self
                    .ctx
                    .job_repo
                    .update_job_status(job_id, JobStatus::Running)
                    .await
                {
                    tracing::error!(
                        job_id = %job_id,
                        host_id = %host_id,
                        vm_id = %vm_id,
                        error = %e,
                        "Failed to persist resumed job; attempting to roll agent back"
                    );

                    if let Err(pause_err) = self.ctx.agent_client.pause_vm(host_id, vm_id).await {
                        tracing::warn!(
                            job_id = %job_id,
                            host_id = %host_id,
                            vm_id = %vm_id,
                            error = %pause_err,
                            "Failed to pause VM while compensating resume persistence failure; stopping VM instead"
                        );
                        if let Err(stop_err) = self.ctx.agent_client.stop_vm(host_id, vm_id).await {
                            tracing::warn!(
                                job_id = %job_id,
                                host_id = %host_id,
                                vm_id = %vm_id,
                                error = %stop_err,
                                "Failed to stop VM while compensating resume persistence failure"
                            );
                        }
                    }

                    return Err(e);
                }

                let mut updated_job = job;
                updated_job.status = JobStatus::Running;
                updated_job.started_at = Some(chrono::Utc::now().timestamp());
                publish_job_update_best_effort(
                    self.ctx.nats_client.as_ref(),
                    &updated_job,
                    "resume-app-job-update",
                )
                .await;

                tracing::info!(job_id = %job_id, "Job resumed successfully");
                Ok(true)
            })
            .await
    }

    pub async fn delete_app(&self, job_id: &str, tenant_id: &str) -> DomainResult<()> {
        let telemetry = self.ctx.telemetry.clone();
        telemetry
            .observe_result("lifecycle", "delete_app", async {
                let job = self.get_app_status(job_id, tenant_id).await?;

                if let (Some(host_id), Some(vm_id)) = (&job.host_id, &job.vm_id)
                    && let Err(e) = self.ctx.agent_client.delete_vm(host_id, vm_id, job.config.hypervisor).await
                {
                    tracing::warn!(
                        job_id = %job_id,
                        host_id = %host_id,
                        vm_id = %vm_id,
                        error = %e,
                        "Best-effort VM deletion failed while removing job"
                    );
                }

                if let Err(e) = self.ctx.job_repo.remove_job(job_id).await {
                    tracing::error!(
                        job_id = %job_id,
                        error = %e,
                        "Failed to remove job after deleting VM; marking job stopped as compensation"
                    );

                    if let Err(compensate_err) = self
                        .ctx
                        .job_repo
                        .update_job_status(job_id, JobStatus::Stopped)
                        .await
                    {
                        tracing::warn!(
                            job_id = %job_id,
                            error = %compensate_err,
                            "Failed to mark job stopped after remove_job failure"
                        );
                    } else {
                        let mut deleted_job = job;
                        deleted_job.status = JobStatus::Stopped;
                        publish_job_update_best_effort(
                            self.ctx.nats_client.as_ref(),
                            &deleted_job,
                            "delete-app-job-update-remove-failed",
                        )
                        .await;
                    }

                    return Err(e);
                }

                let mut deleted_job = job;
                deleted_job.status = JobStatus::Stopped;
                publish_job_update_best_effort(
                    self.ctx.nats_client.as_ref(),
                    &deleted_job,
                    "delete-app-job-update",
                )
                .await;

                Ok(())
            })
            .await
    }

    pub async fn delete_all_by_app(&self, app_id: &str, tenant_id: &str) -> DomainResult<()> {
        let telemetry = self.ctx.telemetry.clone();
        telemetry
            .observe_result("lifecycle", "delete_all_by_app", async {
                let jobs = self
                    .ctx
                    .job_repo
                    .list_jobs(Some(tenant_id), None, None)
                    .await?;
                let app_jobs: Vec<_> = jobs
                    .into_iter()
                    .filter(|job| job.app_id.as_ref() == app_id)
                    .collect();

                for job in &app_jobs {
                    if let (Some(host_id), Some(vm_id)) = (&job.host_id, &job.vm_id)
                        && let Err(e) = self
                            .ctx
                            .agent_client
                            .delete_vm(host_id, vm_id, job.config.hypervisor)
                            .await
                    {
                        let is_missing_vm = matches!(
                            &e,
                            DomainError::Infrastructure(message)
                                if message.to_lowercase().contains("not found")
                        );

                        if is_missing_vm {
                            tracing::warn!(
                                job_id = %job.job_id,
                                host_id = %host_id,
                                vm_id = %vm_id,
                                error = %e,
                                "Best-effort VM deletion skipped missing VM while deleting app"
                            );
                            continue;
                        }

                        tracing::error!(
                            job_id = %job.job_id,
                            host_id = %host_id,
                            vm_id = %vm_id,
                            error = %e,
                            "Failed to delete VM while deleting app; continuing cleanup anyway"
                        );
                    }
                }

                self.ctx.app_repo.remove_app_and_jobs_by_app(app_id).await?;

                for job in app_jobs {
                    let mut deleted_job = job;
                    deleted_job.status = JobStatus::Stopped;
                    publish_job_update_best_effort(
                        self.ctx.nats_client.as_ref(),
                        &deleted_job,
                        "delete-all-by-app-job-update",
                    )
                    .await;
                }

                Ok(())
            })
            .await
    }

    pub async fn cleanup_expired_vms(&self, ttl_secs: i64) -> DomainResult<usize> {
        let telemetry = self.ctx.telemetry.clone();
        telemetry
            .observe_result("lifecycle", "cleanup_expired_vms", async {
                let ttl_secs = ttl_secs.max(1);
                let cutoff = chrono::Utc::now().timestamp() - ttl_secs;
                let jobs = self.ctx.job_repo.list_jobs(None, None, None).await?;

                let expired_jobs: Vec<Job> = jobs
                    .into_iter()
                    .filter(|job| job.vm_id.is_some() && job.created_at <= cutoff)
                    .collect();

                let mut cleaned = 0usize;

                for job in expired_jobs {
                    let now = chrono::Utc::now().timestamp();

                    if let (Some(host_id), Some(vm_id)) = (&job.host_id, &job.vm_id) {
                        match self
                            .ctx
                            .agent_client
                            .delete_vm(host_id, vm_id, job.config.hypervisor)
                            .await
                        {
                            Ok(()) => {},
                            Err(e) => {
                                let is_missing_vm = matches!(
                                    &e,
                                    DomainError::Infrastructure(message)
                                        if message.to_lowercase().contains("not found")
                                );

                                if is_missing_vm {
                                    tracing::warn!(
                                        job_id = %job.job_id,
                                        host_id = %host_id,
                                        vm_id = %vm_id,
                                        error = %e,
                                        "Beta VM cleanup skipped missing VM"
                                    );
                                } else {
                                    tracing::error!(
                                        job_id = %job.job_id,
                                        host_id = %host_id,
                                        vm_id = %vm_id,
                                        error = %e,
                                        "Beta VM cleanup failed while deleting VM"
                                    );
                                    continue;
                                }
                            }
                        }
                    }

                    let mut stopped_job = job.clone();
                    stopped_job.status = JobStatus::Stopped;
                    stopped_job.stopped_at = Some(now);

                    match self.ctx.job_repo.remove_job(&job.job_id).await {
                        Ok(()) => {
                            publish_job_update_best_effort(
                                self.ctx.nats_client.as_ref(),
                                &stopped_job,
                                "beta-vm-cleanup-job-update",
                            )
                            .await;
                            cleaned += 1;
                        },
                        Err(e) => {
                            tracing::error!(
                                job_id = %job.job_id,
                                error = %e,
                                "Failed to remove expired VM job during beta cleanup"
                            );
                            if let Err(update_err) = self
                                .ctx
                                .job_repo
                                .update_job_status(&job.job_id, JobStatus::Stopped)
                                .await
                            {
                                tracing::warn!(
                                    job_id = %job.job_id,
                                    error = %update_err,
                                    "Failed to mark expired VM job as stopped after remove_job failure"
                                );
                            }
                            publish_job_update_best_effort(
                                self.ctx.nats_client.as_ref(),
                                &stopped_job,
                                "beta-vm-cleanup-job-update-remove-failed",
                            )
                            .await;
                        },
                    }
                }

                Ok(cleaned)
            })
            .await
    }

    pub async fn cleanup_beta_deployments(&self) -> DomainResult<usize> {
        let telemetry = self.ctx.telemetry.clone();
        telemetry
            .observe_result("lifecycle", "cleanup_beta_deployments", async {
                let jobs = self.ctx.job_repo.list_jobs(None, None, None).await?;
                let deployment_jobs: Vec<Job> = jobs
                    .into_iter()
                    .filter(|job| job.deployment_id.is_some())
                    .collect();

                let mut cleaned = 0usize;

                for job in deployment_jobs {
                    let now = chrono::Utc::now().timestamp();

                    if let (Some(host_id), Some(vm_id)) = (&job.host_id, &job.vm_id) {
                        match self
                            .ctx
                            .agent_client
                            .delete_vm(host_id, vm_id, job.config.hypervisor)
                            .await
                        {
                            Ok(()) => {},
                            Err(e) => {
                                let is_missing_vm = matches!(
                                    &e,
                                    DomainError::Infrastructure(message)
                                        if message.to_lowercase().contains("not found")
                                );

                                if is_missing_vm {
                                    tracing::warn!(
                                        job_id = %job.job_id,
                                        deployment_id = %job.deployment_id.clone().unwrap_or_default(),
                                        host_id = %host_id,
                                        vm_id = %vm_id,
                                        error = %e,
                                        "Beta deployment cleanup skipped missing VM"
                                    );
                                } else {
                                    tracing::error!(
                                        job_id = %job.job_id,
                                        deployment_id = %job.deployment_id.clone().unwrap_or_default(),
                                        host_id = %host_id,
                                        vm_id = %vm_id,
                                        error = %e,
                                        "Beta deployment cleanup failed while deleting VM"
                                    );
                                    continue;
                                }
                            }
                        }
                    }

                    let mut stopped_job = job.clone();
                    stopped_job.status = JobStatus::Stopped;
                    stopped_job.stopped_at = Some(now);

                    match self.ctx.job_repo.remove_job(&job.job_id).await {
                        Ok(()) => {
                            publish_job_update_best_effort(
                                self.ctx.nats_client.as_ref(),
                                &stopped_job,
                                "beta-deployment-cleanup-job-update",
                            )
                            .await;
                            cleaned += 1;
                        },
                        Err(e) => {
                            tracing::error!(
                                job_id = %job.job_id,
                                deployment_id = %job.deployment_id.clone().unwrap_or_default(),
                                error = %e,
                                "Failed to remove deployment job during beta cleanup"
                            );
                            if let Err(update_err) = self
                                .ctx
                                .job_repo
                                .update_job_status(&job.job_id, JobStatus::Stopped)
                                .await
                            {
                                tracing::warn!(
                                    job_id = %job.job_id,
                                    deployment_id = %job.deployment_id.clone().unwrap_or_default(),
                                    error = %update_err,
                                    "Failed to mark deployment job as stopped after remove_job failure"
                                );
                            }
                            publish_job_update_best_effort(
                                self.ctx.nats_client.as_ref(),
                                &stopped_job,
                                "beta-deployment-cleanup-job-update-remove-failed",
                            )
                            .await;
                        },
                    }
                }

                Ok(cleaned)
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{AppContext, SchedulerRuntimeConfig};
    use crate::domain::app::MockAppRepository;
    use crate::domain::worker::{MockAgentClient, MockJobRepository, MockWorkerRepository};
    use crate::domain::{TenantId, VmConfig, Volume};
    use async_trait::async_trait;
    use std::sync::Arc;
    use uuid::Uuid;

    struct TestNatsPublisher;

    #[async_trait]
    impl crate::application::NatsPublisher for TestNatsPublisher {
        async fn publish(&self, _subject: String, _payload: Vec<u8>) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn test_job(
        job_id: &str,
        created_at: i64,
        status: JobStatus,
        deployment_id: Option<&str>,
    ) -> Job {
        let mut job = Job::new(
            job_id.to_string().into(),
            Uuid::new_v4().to_string().into(),
            "test-app".to_string(),
            "registry.example.com/app:latest".to_string(),
            VmConfig {
                workload_type: crate::domain::job::WorkloadType::App,
                volumes: vec![Volume {
                    volume_id: uuid::Uuid::new_v4().to_string().into(),
                    size_mib: 512,
                    read_only: false,
                    pool_name: "rbd".to_string(),
                    mount_point: "/data".to_string(),
                    access_mode: crate::domain::job::AccessMode::ReadWriteOnce,
                }],
                ..VmConfig::default()
            },
            TenantId::from("tenant-1"),
            deployment_id.map(|id| id.to_string().into()),
        );
        job.created_at = created_at;
        job.status = status;
        job.host_id = Some(crate::domain::HostId::from("host-1"));
        job.vm_id = Some(crate::domain::VmId::from("vm-1"));
        job
    }

    #[tokio::test]
    async fn cleanup_expired_vms_deletes_only_old_jobs_with_vms() {
        let mut job_repo = MockJobRepository::new();
        let mut agent_client = MockAgentClient::new();
        let app_repo = MockAppRepository::new();
        let worker_repo = MockWorkerRepository::new();

        let now = chrono::Utc::now().timestamp();
        let old_job = test_job("job-old", now - 4000, JobStatus::Running, None);
        let recent_job = test_job("job-recent", now - 60, JobStatus::Running, None);

        job_repo
            .expect_list_jobs()
            .withf(|tenant_id, app_id, status| {
                tenant_id.is_none() && app_id.is_none() && status.is_none()
            })
            .returning(move |_, _, _| Ok(vec![old_job.clone(), recent_job.clone()]));

        job_repo
            .expect_remove_job()
            .withf(|job_id| job_id == "job-old")
            .returning(|_| Ok(()));

        agent_client
            .expect_delete_vm()
            .withf(|host_id, vm_id, _| host_id == "host-1" && vm_id == "vm-1")
            .returning(|_, _, _| Ok(()));

        let state = Arc::new(AppContext {
            job_repo: Arc::new(job_repo),
            app_repo: Arc::new(app_repo),
            worker_repo: Arc::new(worker_repo),
            agent_client: Arc::new(agent_client),
            nats_client: Arc::new(TestNatsPublisher),
            telemetry: crate::infrastructure::telemetry::SchedulerTelemetry,
            runtime: SchedulerRuntimeConfig {
                router_idle_timeout_secs: 900,
                worker_stale_threshold_secs: 60,
                restore_retry_backoff_secs: 3600,
            },
        });

        let service = JobLifecycleService::new(state);
        let deleted = service.cleanup_expired_vms(3600).await.unwrap();

        assert_eq!(deleted, 1);
    }

    #[tokio::test]
    async fn cleanup_expired_vms_keeps_job_when_vm_delete_fails() {
        let mut job_repo = MockJobRepository::new();
        let mut agent_client = MockAgentClient::new();
        let app_repo = MockAppRepository::new();
        let worker_repo = MockWorkerRepository::new();

        let now = chrono::Utc::now().timestamp();
        let expired_job = test_job("job-expired", now - 4000, JobStatus::Running, None);

        job_repo
            .expect_list_jobs()
            .withf(|tenant_id, app_id, status| {
                tenant_id.is_none() && app_id.is_none() && status.is_none()
            })
            .returning(move |_, _, _| Ok(vec![expired_job.clone()]));

        agent_client
            .expect_delete_vm()
            .withf(|host_id, vm_id, _| host_id == "host-1" && vm_id == "vm-1")
            .returning(|_, _, _| {
                Err(DomainError::Infrastructure(
                    "temporary agent failure".to_string(),
                ))
            });

        job_repo.expect_remove_job().times(0);
        job_repo.expect_update_job_status().times(0);

        let state = Arc::new(AppContext {
            job_repo: Arc::new(job_repo),
            app_repo: Arc::new(app_repo),
            worker_repo: Arc::new(worker_repo),
            agent_client: Arc::new(agent_client),
            nats_client: Arc::new(TestNatsPublisher),
            telemetry: crate::infrastructure::telemetry::SchedulerTelemetry,
            runtime: SchedulerRuntimeConfig {
                router_idle_timeout_secs: 900,
                worker_stale_threshold_secs: 60,
                restore_retry_backoff_secs: 3600,
            },
        });

        let service = JobLifecycleService::new(state);
        let deleted = service.cleanup_expired_vms(3600).await.unwrap();

        assert_eq!(deleted, 0);
    }

    #[tokio::test]
    async fn cleanup_beta_deployments_deletes_all_deployment_jobs() {
        let mut job_repo = MockJobRepository::new();
        let mut agent_client = MockAgentClient::new();
        let app_repo = MockAppRepository::new();
        let worker_repo = MockWorkerRepository::new();

        let now = chrono::Utc::now().timestamp();
        let deployment_job = test_job(
            "job-deployment",
            now - 120,
            JobStatus::Running,
            Some("dep-1"),
        );
        let non_deployment_job = test_job("job-nondeployment", now - 120, JobStatus::Running, None);

        job_repo
            .expect_list_jobs()
            .withf(|tenant_id, app_id, status| {
                tenant_id.is_none() && app_id.is_none() && status.is_none()
            })
            .returning(move |_, _, _| Ok(vec![deployment_job.clone(), non_deployment_job.clone()]));

        job_repo
            .expect_remove_job()
            .withf(|job_id| job_id == "job-deployment")
            .returning(|_| Ok(()));

        agent_client
            .expect_delete_vm()
            .withf(|host_id, vm_id, _| host_id == "host-1" && vm_id == "vm-1")
            .returning(|_, _, _| Ok(()));

        let state = Arc::new(AppContext {
            job_repo: Arc::new(job_repo),
            app_repo: Arc::new(app_repo),
            worker_repo: Arc::new(worker_repo),
            agent_client: Arc::new(agent_client),
            nats_client: Arc::new(TestNatsPublisher),
            telemetry: crate::infrastructure::telemetry::SchedulerTelemetry,
            runtime: SchedulerRuntimeConfig {
                router_idle_timeout_secs: 900,
                worker_stale_threshold_secs: 60,
                restore_retry_backoff_secs: 3600,
            },
        });

        let service = JobLifecycleService::new(state);
        let deleted = service.cleanup_beta_deployments().await.unwrap();

        assert_eq!(deleted, 1);
    }
}
