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
}
