pub mod deployment;

pub use deployment::DeploymentService;

use crate::domain::{
    AgentClient, AppConfig, AppRepository, DomainError, DomainResult, Job, JobRepository,
    JobStatus, Worker, WorkerRepository,
};
use std::sync::Arc;

const AUTOSCALING_SCALE_DOWN_HYSTERESIS_RATIO: f64 = 0.5;

fn autoscale_next_replicas(app: &AppConfig, current_count: u32, avg_cpu: f32, avg_mem: f32) -> u32 {
    let mut desired = current_count;
    let cpu = avg_cpu as f64;
    let mem = avg_mem as f64;

    if cpu > app.cpu_threshold || mem > app.mem_threshold {
        desired = desired.saturating_add(1).min(app.max_replicas);
    } else if cpu < app.cpu_threshold * AUTOSCALING_SCALE_DOWN_HYSTERESIS_RATIO
        && mem < app.mem_threshold * AUTOSCALING_SCALE_DOWN_HYSTERESIS_RATIO
        && desired > app.min_replicas
    {
        desired -= 1;
    }

    desired
}

pub struct AppService {
    pub deployment: DeploymentService,
    pub job_repo: Arc<dyn JobRepository>,
    pub app_repo: Arc<dyn AppRepository>,
    pub worker_repo: Arc<dyn WorkerRepository>,
    pub agent_client: Arc<dyn AgentClient>,
    pub nats_client: async_nats::Client,
    pub pool: sqlx::PgPool,
    pub router_idle_timeout_secs: i64,
}

impl AppService {
    pub fn new(
        job_repo: Arc<dyn JobRepository>,
        app_repo: Arc<dyn AppRepository>,
        worker_repo: Arc<dyn WorkerRepository>,
        agent_client: Arc<dyn AgentClient>,
        nats_client: async_nats::Client,
        pool: sqlx::PgPool,
        router_idle_timeout_secs: i64,
    ) -> Self {
        Self {
            deployment: DeploymentService::new(
                job_repo.clone(),
                worker_repo.clone(),
                agent_client.clone(),
                nats_client.clone(),
            ),
            job_repo,
            app_repo,
            worker_repo,
            agent_client,
            nats_client,
            pool,
            router_idle_timeout_secs,
        }
    }

    pub async fn get_app_status(&self, job_id: &str, user_id: &str) -> DomainResult<Job> {
        let job = self
            .job_repo
            .get_job(job_id)
            .await?
            .ok_or_else(|| DomainError::JobNotFound(job_id.to_string()))?;

        if job.user_id != user_id && user_id != "system" {
            return Err(DomainError::Unauthorized(
                "You do not own this job".to_string(),
            ));
        }

        Ok(job)
    }

    pub async fn pause_app(&self, job_id: &str, user_id: &str) -> DomainResult<()> {
        let job = self.get_app_status(job_id, user_id).await?;

        if matches!(job.status, JobStatus::Paused | JobStatus::Stopped) {
            tracing::info!(
                job_id = %job_id,
                status = %job.status.as_str(),
                "Pause requested for a job that is already paused or stopped"
            );
            return Ok(());
        }

        // Re-check traffic before hibernating to avoid race condition
        if let Ok(Some(app)) = self.app_repo.get_app_config(&job.app_id).await {
            let now = chrono::Utc::now().timestamp();
            if app.last_router_traffic_at > app.last_scaled_to_zero_at
                && now - app.last_router_traffic_at < self.router_idle_timeout_secs
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
            if let Err(e) = self.agent_client.pause_vm(host_id, vm_id).await {
                tracing::warn!(
                    "Pause failed for {}, attempting stop fallback: {}",
                    job_id,
                    e
                );
                self.agent_client.stop_vm(host_id, vm_id).await?;
            }
            self.job_repo
                .update_job_status(job_id, JobStatus::Paused)
                .await?;

            let mut updated_job = job;
            updated_job.status = JobStatus::Paused;
            let _ = self.publish_job_update(&updated_job).await;
        }

        tracing::info!(job_id = %job_id, "Job paused successfully");
        Ok(())
    }

    pub async fn resume_app(&self, job_id: &str, user_id: &str) -> DomainResult<bool> {
        let job = self.get_app_status(job_id, user_id).await?;

        let Some(host_id) = job.host_id.as_ref() else {
            return Ok(false);
        };
        let Some(vm_id) = job.vm_id.as_ref() else {
            return Ok(false);
        };

        if self.worker_repo.get_worker(host_id).await?.is_none() {
            let message = format!("Host {host_id} no longer exists");
            tracing::warn!(
                job_id = %job_id,
                app_id = %job.app_id,
                host_id = %host_id,
                "Invalidating paused job linked to a missing host"
            );

            self.job_repo
                .fail_job(job_id, message.clone(), chrono::Utc::now().timestamp())
                .await?;

            let mut updated_job = job;
            updated_job.status = JobStatus::Failed;
            updated_job.stopped_at = Some(chrono::Utc::now().timestamp());
            updated_job.error_message = Some(message);
            let _ = self.publish_job_update(&updated_job).await;

            return Ok(false);
        }

        tracing::info!(
            job_id = %job_id,
            app_id = %job.app_id,
            vm_id = ?job.vm_id,
            "Resuming job"
        );

        self.agent_client.resume_vm(host_id, vm_id).await?;
        self.job_repo
            .update_job_status(job_id, JobStatus::Running)
            .await?;

        let mut updated_job = job;
        updated_job.status = JobStatus::Running;
        updated_job.started_at = Some(chrono::Utc::now().timestamp());
        let _ = self.publish_job_update(&updated_job).await;

        tracing::info!(job_id = %job_id, "Job resumed successfully");
        Ok(true)
    }

    pub async fn delete_app(&self, job_id: &str, user_id: &str) -> DomainResult<()> {
        let job = self.get_app_status(job_id, user_id).await?;

        if let (Some(host_id), Some(vm_id)) = (&job.host_id, &job.vm_id) {
            let _ = self.agent_client.delete_vm(host_id, vm_id).await;
        }

        self.job_repo.remove_job(job_id).await?;

        let mut deleted_job = job;
        deleted_job.status = JobStatus::Stopped;
        let _ = self.publish_job_update(&deleted_job).await;

        Ok(())
    }

    async fn publish_job_update(&self, job: &Job) -> DomainResult<()> {
        use mikrom_proto::scheduler::AppInfo;
        use prost::Message;

        let info = AppInfo {
            job_id: job.job_id.clone(),
            app_id: job.app_id.clone(),
            app_name: job.app_name.clone(),
            image: job.image.clone(),
            status: job.status as i32,
            host_id: job.host_id.clone().unwrap_or_default(),
            vm_id: job.vm_id.clone().unwrap_or_default(),
            user_id: job.user_id.clone(),
            deployment_id: job.deployment_id.clone().unwrap_or_default(),
            ipv6_address: job.config.ipv6_address.clone().unwrap_or_default(),
            ..Default::default()
        };

        let mut buf = Vec::new();
        if info.encode(&mut buf).is_ok() {
            let _ = self
                .nats_client
                .publish(mikrom_proto::subjects::SCHEDULER_JOB_UPDATES, buf.into())
                .await;
        }

        Ok(())
    }

    pub async fn delete_all_by_app(&self, app_id: &str, user_id: &str) -> DomainResult<()> {
        let jobs = self.job_repo.list_jobs(Some(user_id), None, None).await?;
        let app_jobs: Vec<_> = jobs.into_iter().filter(|j| j.app_id == app_id).collect();
        let mut failures = Vec::new();

        for job in app_jobs {
            #[allow(clippy::collapsible_if)]
            if let (Some(host_id), Some(vm_id)) = (&job.host_id, &job.vm_id) {
                if let Err(e) = self.agent_client.delete_vm(host_id, vm_id).await {
                    let error_text = e.to_string();
                    if Self::is_vm_already_gone(&error_text) {
                        tracing::info!(
                            vm_id = %vm_id,
                            host_id = %host_id,
                            "VM already absent during app cleanup; treating as success"
                        );
                        continue;
                    }

                    tracing::error!("Failed to delete VM {} on host {}: {}", vm_id, host_id, e);
                    failures.push(format!("{} on {}: {}", vm_id, host_id, e));
                }
            }
        }

        if !failures.is_empty() {
            return Err(crate::domain::DomainError::Infrastructure(format!(
                "Failed to delete one or more VMs for app {app_id}: {}",
                failures.join("; ")
            )));
        }

        self.job_repo.remove_jobs_by_app(app_id).await?;
        self.app_repo
            .remove_app_config(app_id)
            .await
            .map_err(|e| DomainError::Infrastructure(e.to_string()))?;
        Ok(())
    }

    pub async fn scale_app(
        &self,
        app_id: &str,
        desired_replicas: u32,
        user_id: &str,
    ) -> DomainResult<()> {
        let jobs = self.job_repo.list_jobs(Some(user_id), None, None).await?;
        let active_jobs: Vec<_> = jobs
            .into_iter()
            .filter(|j| {
                j.app_id == app_id
                    && matches!(
                        j.status,
                        JobStatus::Pending | JobStatus::Scheduled | JobStatus::Running
                    )
            })
            .collect();
        let paused_jobs: Vec<_> = self
            .job_repo
            .list_jobs(Some(user_id), Some(app_id), None)
            .await?
            .into_iter()
            .filter(|j| j.status == JobStatus::Paused)
            .collect();

        let current_count = active_jobs.len() as u32;

        if current_count == desired_replicas {
            return Ok(());
        }

        if current_count < desired_replicas {
            let mut to_add = desired_replicas - current_count;
            tracing::info!(app_id = %app_id, to_add = %to_add, "Scaling up app");

            let mut resumed = 0u32;
            if current_count == 0 && !paused_jobs.is_empty() {
                let mut resume_candidates = paused_jobs.clone();
                resume_candidates.sort_by_key(|b| std::cmp::Reverse(b.created_at));

                for job in resume_candidates.iter().take(to_add as usize) {
                    if job.host_id.is_some() && job.vm_id.is_some() {
                        match self.resume_app(&job.job_id, user_id).await {
                            Ok(true) => {
                                resumed += 1;
                            },
                            Ok(false) => {},
                            Err(e) => return Err(e),
                        }
                    }
                }

                to_add -= resumed;
            }

            if to_add == 0 {
                return Ok(());
            }

            // Fetch app config to get the VPC prefix
            let app_config = self.app_repo.get_app_config(app_id).await?;
            let vpc_prefix = app_config.map(|c| c.vpc_ipv6_prefix).unwrap_or_default();

            // Find a template job to clone. Prefer an active job, then a paused job.
            let mut template_job = active_jobs
                .first()
                .cloned()
                .or_else(|| paused_jobs.first().cloned());

            if template_job.is_none() {
                let mut all_jobs = self
                    .job_repo
                    .list_jobs(Some(user_id), Some(app_id), None)
                    .await?;
                all_jobs.sort_by_key(|b| std::cmp::Reverse(b.created_at));
                template_job = all_jobs.into_iter().next();
            }

            let Some(template_job) = template_job else {
                tracing::debug!(
                    app_id = %app_id,
                    "Scaling up from zero but no preserved deployment was found; leaving app unchanged"
                );
                return Ok(());
            };

            let mut deployment_futures = Vec::new();

            for _ in 0..to_add {
                let deployment = self.deployment.clone();
                let app_id = template_job.app_id.clone();
                let app_name = template_job.app_name.clone();
                let image = template_job.image.clone();
                let user_id = template_job.user_id.clone();
                let deployment_id = template_job.deployment_id.clone().unwrap_or_default();
                let vpc_prefix = vpc_prefix.clone();
                let config = template_job.config.clone();

                deployment_futures.push(async move {
                    deployment
                        .deploy_app(
                            app_id,
                            app_name,
                            image,
                            user_id,
                            deployment_id,
                            vpc_prefix,
                            config,
                            crate::domain::worker::SchedulingStrategy::LeastLoaded,
                        )
                        .await
                });
            }

            let results = futures::future::join_all(deployment_futures).await;
            let errors: Vec<_> = results.into_iter().filter_map(|r| r.err()).collect();

            if !errors.is_empty() {
                tracing::error!(
                    app_id = %app_id,
                    failed = %errors.len(),
                    total = %to_add,
                    "Some scale-up deployments failed"
                );
                return Err(DomainError::Infrastructure(format!(
                    "Failed to deploy {}/{} replicas: {:?}",
                    errors.len(),
                    to_add,
                    errors[0]
                )));
            }
        } else {
            let to_remove = current_count - desired_replicas;
            tracing::info!(app_id = %app_id, to_remove = %to_remove, "Scaling down app");

            // Sort jobs by status: Pending first, then Scheduled, then Running
            let mut jobs_to_kill = active_jobs;
            jobs_to_kill.sort_by_key(|j| match j.status {
                JobStatus::Pending => 0,
                JobStatus::Scheduled => 1,
                JobStatus::Running => 2,
                _ => 3,
            });

            for job in jobs_to_kill.iter().take(to_remove as usize) {
                if desired_replicas == 0 {
                    if job.host_id.is_some() && job.vm_id.is_some() {
                        self.pause_app(&job.job_id, user_id).await?;
                    } else {
                        self.delete_app(&job.job_id, user_id).await?;
                    }
                } else {
                    self.delete_app(&job.job_id, user_id).await?;
                }
            }
        }

        Ok(())
    }

    pub async fn start_autoscaler(self: Arc<Self>) {
        tracing::info!("Starting background autoscaler loop");
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));

        loop {
            interval.tick().await;
            if let Err(e) = self.reconcile_apps().await {
                tracing::error!("App reconciliation failed: {}", e);
            }
        }
    }

    pub async fn reconcile_apps(&self) -> DomainResult<()> {
        // 1. Get all app configurations
        let apps = self
            .app_repo
            .list_all_apps()
            .await
            .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

        if apps.is_empty() {
            return Ok(());
        }

        // 2. Get all active jobs grouped by app_id (including Pending/Scheduled)
        let all_jobs = self
            .job_repo
            .list_jobs(None, None, None)
            .await?
            .into_iter()
            .filter(|j| {
                matches!(
                    j.status,
                    JobStatus::Pending | JobStatus::Scheduled | JobStatus::Running
                )
            })
            .collect::<Vec<_>>();

        // Optimization: Fetch all workers once to avoid N+1 queries in the loop
        let workers = self
            .worker_repo
            .list_workers()
            .await
            .map_err(|e| DomainError::Infrastructure(e.to_string()))?;
        let worker_map: std::collections::HashMap<String, Worker> = workers
            .into_iter()
            .map(|w| (w.host_id.clone(), w))
            .collect();

        let mut app_running_counts: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();
        let mut app_metrics: std::collections::HashMap<String, (f32, f32)> =
            std::collections::HashMap::new();

        for job in all_jobs {
            let count = app_running_counts.entry(job.app_id.clone()).or_insert(0);
            *count += 1;

            let vm_metrics = job
                .host_id
                .as_ref()
                .and_then(|h| job.vm_id.as_ref().map(|v| (h, v)))
                .and_then(|(h, _v)| worker_map.get(h))
                .and_then(|w| w.metrics.as_ref())
                .and_then(|m| m.vms.get(job.vm_id.as_ref().unwrap()));

            if let Some(vm_metrics) = vm_metrics {
                let entry = app_metrics.entry(job.app_id.clone()).or_insert((0.0, 0.0));
                entry.0 += vm_metrics.cpu_usage;
                entry.1 += (vm_metrics.ram_used_bytes as f32
                    / job.config.memory_mib as f32
                    / 1024.0
                    / 1024.0)
                    * 100.0;
            }
        }

        let now = chrono::Utc::now().timestamp();

        // 3. Evaluate each app
        for mut app in apps {
            let current_count = *app_running_counts.get(&app.id).unwrap_or(&0);

            if current_count > 0
                && app.min_replicas == 0
                && app.desired_replicas > 0
                && app.last_router_traffic_at == 0
            {
                let updated_app = AppConfig {
                    last_router_traffic_at: now,
                    ..app.clone()
                };

                if let Err(e) = self.app_repo.update_app_config(updated_app).await {
                    tracing::error!(
                        app_id = %app.id,
                        error = %e,
                        "Failed to initialize router traffic timestamp for active app"
                    );
                    continue;
                }

                app.last_router_traffic_at = now;
            }

            let router_idle = app.min_replicas == 0
                && app.last_router_traffic_at > 0
                && now - app.last_router_traffic_at >= self.router_idle_timeout_secs;
            let should_restore_from_zero = app.min_replicas == 0
                && app.desired_replicas > 0
                && app.last_router_traffic_at > app.last_scaled_to_zero_at;

            if current_count > 0 && router_idle {
                tracing::info!(
                    event = "scale_to_zero",
                    app_id = %app.id,
                    last_router_traffic_at = %app.last_router_traffic_at,
                    timeout_secs = self.router_idle_timeout_secs,
                    "No router traffic for configured idle timeout; scaling app to zero"
                );

                // Update timestamp FIRST to prevent immediate wake-up if scale_app takes time or fails
                if let Err(e) = self
                    .app_repo
                    .update_app_config(AppConfig {
                        last_scaled_to_zero_at: now,
                        ..app.clone()
                    })
                    .await
                {
                    tracing::error!(
                        app_id = %app.id,
                        error = %e,
                        "Failed to persist scale-to-zero timestamp"
                    );
                    // If we can't persist the timestamp, we shouldn't scale down yet
                    // because we won't be able to detect NEW traffic correctly.
                    continue;
                }

                if let Err(e) = self.scale_app(&app.id, 0, &app.user_id).await {
                    tracing::error!(
                        app_id = %app.id,
                        error = %e,
                        "Failed to scale app to zero after router inactivity"
                    );
                }

                continue;
            }

            if current_count == 0 {
                // If it's a new app or has never reached zero correctly, initialize the timestamp
                if app.last_scaled_to_zero_at == 0 && app.last_router_traffic_at == 0 {
                    let _ = self
                        .app_repo
                        .update_app_config(AppConfig {
                            last_scaled_to_zero_at: now,
                            ..app.clone()
                        })
                        .await;
                    continue;
                }

                if should_restore_from_zero {
                    tracing::info!(
                        event = "restore_from_router_traffic",
                        app_id = %app.id,
                        desired = %app.desired_replicas,
                        "Restoring app after router traffic returned"
                    );

                    if let Err(e) = self
                        .scale_app(&app.id, app.desired_replicas, &app.user_id)
                        .await
                    {
                        tracing::error!(
                            app_id = %app.id,
                            desired = %app.desired_replicas,
                            error = %e,
                            "Failed to restore app after router traffic"
                        );
                    }
                } else if app.min_replicas > 0 {
                    // App has no running instances but min_replicas > 0, scale up to min
                    tracing::info!(app_id = %app.id, "Scaling up to min_replicas");
                    if let Err(e) = self
                        .scale_app(&app.id, app.min_replicas, &app.user_id)
                        .await
                    {
                        tracing::error!("Failed to scale app {} to min: {}", app.id, e);
                    } else if let Err(e) = self
                        .app_repo
                        .update_app_config(AppConfig {
                            desired_replicas: app.min_replicas,
                            ..app.clone()
                        })
                        .await
                    {
                        tracing::error!(
                            app_id = %app.id,
                            desired = %app.min_replicas,
                            error = %e,
                            "Failed to persist min_replicas target"
                        );
                    }
                } else if app.min_replicas == 0 {
                    // If min_replicas is 0, we stay at 0 until traffic returns.
                    // Do nothing.
                } else if !app.autoscaling_enabled && app.desired_replicas > 0 {
                    // This is handled by the first deployment usually, but if all instances died, we might want to recover.
                    // However, we need a template job. scale_app handles that.
                    tracing::debug!(app_id = %app.id, "App has 0 instances but desired > 0. Waiting for first deploy or historical template.");
                    // We try scale_app anyway, it will fail if no template exists.
                    let _ = self
                        .scale_app(&app.id, app.desired_replicas, &app.user_id)
                        .await;
                }

                continue;
            }

            if app.autoscaling_enabled {
                if let Some((total_cpu, total_mem)) = app_metrics.get(&app.id) {
                    let avg_cpu = total_cpu / (current_count as f32);
                    let avg_mem = total_mem / (current_count as f32);

                    tracing::debug!(
                        app_id = %app.id,
                        avg_cpu = %avg_cpu,
                        avg_mem = %avg_mem,
                        count = %current_count,
                        "Evaluating autoscaling"
                    );

                    let desired = autoscale_next_replicas(&app, current_count, avg_cpu, avg_mem);

                    if desired > current_count {
                        tracing::info!(
                            app_id = %app.id,
                            avg_cpu = %avg_cpu,
                            avg_mem = %avg_mem,
                            "Scale up triggered (auto)"
                        );
                    } else if desired < current_count {
                        tracing::info!(
                            app_id = %app.id,
                            avg_cpu = %avg_cpu,
                            avg_mem = %avg_mem,
                            "Scale down triggered (auto)"
                        );
                    }

                    if desired != current_count {
                        if let Err(e) = self.scale_app(&app.id, desired, &app.user_id).await {
                            tracing::error!(
                                app_id = %app.id,
                                desired = %desired,
                                error = %e,
                                "Failed to reconcile autoscaling"
                            );
                        } else if let Err(e) = self
                            .app_repo
                            .update_app_config(AppConfig {
                                desired_replicas: desired,
                                ..app.clone()
                            })
                            .await
                        {
                            tracing::error!(
                                app_id = %app.id,
                                desired = %desired,
                                error = %e,
                                "Failed to persist autoscaling target"
                            );
                        }
                    }
                }
            } else {
                // Manual scaling reconciliation
                if current_count != app.desired_replicas {
                    tracing::info!(
                        app_id = %app.id,
                        current = %current_count,
                        desired = %app.desired_replicas,
                        "Reconciling manual scaling"
                    );
                    if let Err(e) = self
                        .scale_app(&app.id, app.desired_replicas, &app.user_id)
                        .await
                    {
                        tracing::error!("Failed to reconcile app {}: {}", app.id, e);
                    }
                }
            }
        }

        Ok(())
    }

    async fn resolve_storage_host(&self, host_id: &str) -> DomainResult<String> {
        if !host_id.is_empty() {
            return Ok(host_id.to_string());
        }

        self.pick_any_healthy_worker().await
    }

    async fn resolve_volume_host(&self, host_id: &str) -> DomainResult<String> {
        if !host_id.is_empty() {
            return Ok(host_id.to_string());
        }

        self.pick_any_healthy_worker().await
    }

    fn is_vm_already_gone(error_text: &str) -> bool {
        let normalized = error_text.to_lowercase();
        normalized.contains("vm not found")
    }

    pub async fn check_health(&self, job_id: &str, user_id: &str) -> DomainResult<bool> {
        let job = self.get_app_status(job_id, user_id).await?;
        if let (Some(host_id), Some(vm_id)) = (&job.host_id, &job.vm_id) {
            self.agent_client.check_health(host_id, vm_id).await
        } else {
            Ok(false)
        }
    }

    pub async fn update_security_groups(
        &self,
        _req: mikrom_proto::scheduler::UpdateSecurityGroupsRequest,
    ) -> DomainResult<()> {
        // ... (rest of implementation) ...
        Ok(())
    }

    pub async fn create_volume(
        &self,
        host_id: &str,
        volume_id: &str,
        size_mib: u32,
        pool_name: &str,
    ) -> DomainResult<()> {
        let target_host = self.resolve_volume_host(host_id).await?;

        self.agent_client
            .create_volume(&target_host, volume_id, size_mib, pool_name)
            .await
    }

    pub async fn create_snapshot(
        &self,
        host_id: &str,
        volume_id: &str,
        snapshot_name: &str,
        pool_name: &str,
    ) -> DomainResult<()> {
        let target_host = self.resolve_storage_host(host_id).await?;

        self.agent_client
            .create_snapshot(&target_host, volume_id, snapshot_name, pool_name)
            .await
    }

    pub async fn delete_volume(
        &self,
        host_id: &str,
        volume_id: &str,
        pool_name: &str,
    ) -> DomainResult<()> {
        let target_host = self.resolve_storage_host(host_id).await?;

        self.agent_client
            .delete_volume(&target_host, volume_id, pool_name)
            .await
    }

    pub async fn delete_snapshot(
        &self,
        host_id: &str,
        volume_id: &str,
        snapshot_name: &str,
        pool_name: &str,
    ) -> DomainResult<()> {
        let target_host = self.resolve_storage_host(host_id).await?;

        self.agent_client
            .delete_snapshot(&target_host, volume_id, snapshot_name, pool_name)
            .await
    }

    pub async fn restore_snapshot(
        &self,
        host_id: &str,
        volume_id: &str,
        snapshot_name: &str,
        pool_name: &str,
    ) -> DomainResult<()> {
        let target_host = self.resolve_storage_host(host_id).await?;

        self.agent_client
            .restore_snapshot(&target_host, volume_id, snapshot_name, pool_name)
            .await
    }

    pub async fn clone_volume(
        &self,
        host_id: &str,
        source_volume_id: &str,
        snapshot_name: &str,
        target_volume_id: &str,
        pool_name: &str,
    ) -> DomainResult<()> {
        let target_host = self.resolve_storage_host(host_id).await?;

        self.agent_client
            .clone_volume(
                &target_host,
                source_volume_id,
                snapshot_name,
                target_volume_id,
                pool_name,
            )
            .await
    }

    async fn pick_any_healthy_worker(&self) -> DomainResult<String> {
        let workers = self.worker_repo.get_available_workers(30).await?;
        if let Some(w) = workers.first() {
            return Ok(w.host_id.clone());
        }

        // Fallback: Try any worker that has sent a heartbeat recently, even if it hasn't sent metrics yet
        let all_workers = self.worker_repo.list_workers().await?;
        let now = chrono::Utc::now().timestamp();
        let fallback = all_workers
            .iter()
            .filter(|w| now - w.last_heartbeat < 30)
            .max_by_key(|w| w.last_heartbeat);

        fallback
            .map(|w| w.host_id.clone())
            .ok_or_else(|| DomainError::Infrastructure("No healthy workers available for storage operation. Ensure agents are running and connected to NATS.".to_string()))
    }

    pub async fn get_job_metrics(&self, job: &Job) -> (f32, u64, u64, u64) {
        let metrics = async {
            let host_id = job.host_id.as_ref()?;
            let worker = self.worker_repo.get_worker(host_id).await.ok()??;
            let metrics = worker.metrics.as_ref()?;
            let vm_id = job.vm_id.as_ref()?;
            metrics
                .vms
                .get(vm_id)
                .map(|m| (m.cpu_usage, m.ram_used_bytes, m.tx_bytes, m.rx_bytes))
        }
        .await;

        metrics.unwrap_or((0.0, 0, 0, 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::job::{Job, JobStatus, VmConfig};
    use crate::domain::worker::{MockAgentClient, MockJobRepository, MockWorkerRepository};
    use crate::domain::{
        AgentClient, AppConfig, AppRepository, DomainError, DomainResult, JobRepository, Worker,
        WorkerRepository,
    };
    use async_trait::async_trait;
    use mockall::predicate::eq;
    use std::sync::Arc;

    struct DummyJobRepo {
        job: Job,
    }

    #[async_trait]
    impl JobRepository for DummyJobRepo {
        async fn add_job(&self, _job: Job) -> DomainResult<()> {
            Ok(())
        }
        async fn get_job(&self, _job_id: &str) -> DomainResult<Option<Job>> {
            Ok(Some(self.job.clone()))
        }
        async fn update_job_status(&self, _job_id: &str, _status: JobStatus) -> DomainResult<()> {
            Ok(())
        }
        async fn start_job(&self, _job_id: &str, _ts: i64) -> DomainResult<()> {
            Ok(())
        }
        async fn fail_job(&self, _job_id: &str, _msg: String, _ts: i64) -> DomainResult<()> {
            Ok(())
        }
        async fn cancel_job(&self, _job_id: &str, _ts: i64) -> DomainResult<()> {
            Ok(())
        }
        async fn remove_job(&self, _j: &str) -> DomainResult<()> {
            Ok(())
        }
        async fn remove_jobs_by_app(&self, _app: &str) -> DomainResult<()> {
            Ok(())
        }
        async fn list_jobs<'a>(
            &self,
            _u: Option<&'a str>,
            _a: Option<&'a str>,
            _s: Option<JobStatus>,
        ) -> DomainResult<Vec<Job>> {
            Ok(vec![])
        }
        async fn find_job_by_vm_id(&self, _v: &str) -> DomainResult<Option<Job>> {
            Ok(None)
        }
    }

    struct DummyWorkerRepo;
    #[async_trait]
    impl WorkerRepository for DummyWorkerRepo {
        async fn register(&self, _w: Worker) -> DomainResult<()> {
            Ok(())
        }
        async fn unregister(&self, _h: &str) -> DomainResult<()> {
            Ok(())
        }
        async fn update_metrics(
            &self,
            _h: &str,
            _m: crate::domain::HostMetrics,
        ) -> DomainResult<()> {
            Ok(())
        }
        async fn get_worker(&self, _h: &str) -> DomainResult<Option<Worker>> {
            Ok(None)
        }
        async fn list_workers(&self) -> DomainResult<Vec<Worker>> {
            Ok(vec![])
        }
        async fn get_available_workers(&self, _t: i64) -> DomainResult<Vec<Worker>> {
            Ok(vec![])
        }
    }

    struct DummyAppRepo;
    #[async_trait]
    impl AppRepository for DummyAppRepo {
        async fn update_app_config(&self, _config: AppConfig) -> anyhow::Result<()> {
            Ok(())
        }
        async fn get_app_config(&self, _: &str) -> anyhow::Result<Option<AppConfig>> {
            Ok(None)
        }
        async fn get_app_config_by_hostname(&self, _: &str) -> anyhow::Result<Option<AppConfig>> {
            Ok(None)
        }
        async fn list_all_apps(&self) -> anyhow::Result<Vec<AppConfig>> {
            Ok(vec![])
        }
        async fn list_autoscaling_apps(&self) -> anyhow::Result<Vec<AppConfig>> {
            Ok(vec![])
        }
        async fn remove_app_config(&self, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct DummyAgentClient {
        healthy: bool,
    }

    #[async_trait]
    impl AgentClient for DummyAgentClient {
        async fn update_firewall(
            &self,
            _host_id: &str,
            _vm_id: &str,
            _rules: Vec<mikrom_proto::scheduler::FirewallRule>,
        ) -> DomainResult<()> {
            Ok(())
        }

        async fn start_vm(
            &self,
            _h: &str,
            _a: &str,
            _i: &str,
            _v: &str,
            _c: &VmConfig,
        ) -> DomainResult<()> {
            Ok(())
        }
        async fn pause_vm(&self, _h: &str, _v: &str) -> DomainResult<()> {
            Ok(())
        }
        async fn resume_vm(&self, _h: &str, _v: &str) -> DomainResult<()> {
            Ok(())
        }
        async fn stop_vm(&self, _h: &str, _v: &str) -> DomainResult<()> {
            Ok(())
        }
        async fn delete_vm(&self, _h: &str, _v: &str) -> DomainResult<()> {
            Ok(())
        }
        async fn check_health(&self, _h: &str, _v: &str) -> DomainResult<bool> {
            Ok(self.healthy)
        }

        async fn create_volume(
            &self,
            _host_id: &str,
            _volume_id: &str,
            _size_mib: u32,
            _pool_name: &str,
        ) -> DomainResult<()> {
            Ok(())
        }

        async fn create_snapshot(
            &self,
            _host_id: &str,
            _volume_id: &str,
            _snapshot_name: &str,
            _pool_name: &str,
        ) -> DomainResult<()> {
            Ok(())
        }

        async fn delete_volume(
            &self,
            _host_id: &str,
            _volume_id: &str,
            _pool_name: &str,
        ) -> DomainResult<()> {
            Ok(())
        }

        async fn delete_snapshot(
            &self,
            _host_id: &str,
            _volume_id: &str,
            _snapshot_name: &str,
            _pool_name: &str,
        ) -> DomainResult<()> {
            Ok(())
        }

        async fn restore_snapshot(
            &self,
            _host_id: &str,
            _volume_id: &str,
            _snapshot_name: &str,
            _pool_name: &str,
        ) -> DomainResult<()> {
            Ok(())
        }

        async fn clone_volume(
            &self,
            _host_id: &str,
            _source_volume_id: &str,
            _snapshot_name: &str,
            _target_volume_id: &str,
            _pool_name: &str,
        ) -> DomainResult<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_check_health_dispatch() {
        let mut job = Job::new(
            "job-1".to_string(),
            "app-1".to_string(),
            "app1".to_string(),
            "img".to_string(),
            VmConfig::default(),
            "user-1".to_string(),
            None,
        );
        job.schedule("host-1".to_string(), "vm-1".to_string());

        let job_repo = Arc::new(DummyJobRepo { job });
        let app_repo = Arc::new(DummyAppRepo);
        let worker_repo = Arc::new(DummyWorkerRepo);
        let agent_client = Arc::new(DummyAgentClient { healthy: true });

        // Use a lazy pool that doesn't connect for testing
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();

        let nats_client = async_nats::connect("nats://localhost:4223").await.unwrap();
        let service = AppService {
            deployment: DeploymentService::new(
                job_repo.clone(),
                worker_repo.clone(),
                agent_client.clone(),
                nats_client.clone(),
            ),
            job_repo,
            app_repo,
            worker_repo,
            agent_client,
            nats_client,
            pool,
            router_idle_timeout_secs: 900,
        };

        let res = service.check_health("job-1", "user-1").await.unwrap();
        assert!(res);
    }

    fn paused_job() -> Job {
        let mut job = Job::new(
            "job-1".to_string(),
            "app-1".to_string(),
            "app1".to_string(),
            "img".to_string(),
            VmConfig::default(),
            "user-1".to_string(),
            None,
        );
        job.schedule("host-1".to_string(), "vm-1".to_string());
        job.status = JobStatus::Running;
        job
    }

    #[tokio::test]
    async fn test_pause_app_success_updates_status_without_stop() {
        let job = paused_job();
        let mut job_repo = MockJobRepository::new();
        job_repo.expect_get_job().with(eq("job-1")).returning({
            let job = job.clone();
            move |_| Ok(Some(job.clone()))
        });
        job_repo
            .expect_update_job_status()
            .with(eq("job-1"), eq(JobStatus::Paused))
            .times(1)
            .returning(|_, _| Ok(()));

        let mut agent_client = MockAgentClient::new();
        agent_client
            .expect_pause_vm()
            .with(eq("host-1"), eq("vm-1"))
            .times(1)
            .returning(|_, _| Ok(()));
        agent_client.expect_stop_vm().times(0);

        let worker_repo = Arc::new(MockWorkerRepository::new());
        let app_repo = Arc::new(DummyAppRepo);
        let job_repo = Arc::new(job_repo);
        let agent_client = Arc::new(agent_client);
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();

        let nats_client = async_nats::connect("nats://localhost:4223").await.unwrap();
        let service = AppService {
            deployment: DeploymentService::new(
                job_repo.clone(),
                worker_repo.clone(),
                agent_client.clone(),
                nats_client.clone(),
            ),
            job_repo,
            app_repo,
            worker_repo,
            agent_client,
            nats_client,
            pool,
            router_idle_timeout_secs: 900,
        };

        service.pause_app("job-1", "user-1").await.unwrap();
    }

    #[tokio::test]
    async fn test_pause_app_fallback_stops_vm_on_pause_failure() {
        let job = paused_job();
        let mut job_repo = MockJobRepository::new();
        job_repo.expect_get_job().with(eq("job-1")).returning({
            let job = job.clone();
            move |_| Ok(Some(job.clone()))
        });
        job_repo
            .expect_update_job_status()
            .with(eq("job-1"), eq(JobStatus::Paused))
            .times(1)
            .returning(|_, _| Ok(()));

        let mut agent_client = MockAgentClient::new();
        agent_client
            .expect_pause_vm()
            .with(eq("host-1"), eq("vm-1"))
            .times(1)
            .returning(|_, _| Err(DomainError::Infrastructure("boom".to_string())));
        agent_client
            .expect_stop_vm()
            .with(eq("host-1"), eq("vm-1"))
            .times(1)
            .returning(|_, _| Ok(()));

        let worker_repo = Arc::new(MockWorkerRepository::new());
        let app_repo = Arc::new(DummyAppRepo);
        let job_repo = Arc::new(job_repo);
        let agent_client = Arc::new(agent_client);
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();

        let nats_client = async_nats::connect("nats://localhost:4223").await.unwrap();
        let service = AppService {
            deployment: DeploymentService::new(
                job_repo.clone(),
                worker_repo.clone(),
                agent_client.clone(),
                nats_client.clone(),
            ),
            job_repo,
            app_repo,
            worker_repo,
            agent_client,
            nats_client,
            pool,
            router_idle_timeout_secs: 900,
        };

        service.pause_app("job-1", "user-1").await.unwrap();
    }

    #[tokio::test]
    async fn test_delete_all_by_app_treats_missing_vm_as_success() {
        let job = paused_job();
        let mut job_repo = MockJobRepository::new();
        job_repo
            .expect_list_jobs()
            .returning(move |_, _, _| Ok(vec![job.clone()]));
        job_repo
            .expect_remove_jobs_by_app()
            .with(eq("app-1"))
            .times(1)
            .returning(|_| Ok(()));

        let mut agent_client = MockAgentClient::new();
        agent_client
            .expect_delete_vm()
            .with(eq("host-1"), eq("vm-1"))
            .times(1)
            .returning(|_, _| {
                Err(DomainError::Infrastructure(
                    "VM not found: vm-1".to_string(),
                ))
            });

        let worker_repo = Arc::new(MockWorkerRepository::new());
        let app_repo = Arc::new(DummyAppRepo);
        let job_repo = Arc::new(job_repo);
        let agent_client = Arc::new(agent_client);
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();

        let nats_client = async_nats::connect("nats://localhost:4223").await.unwrap();
        let service = AppService {
            deployment: DeploymentService::new(
                job_repo.clone(),
                worker_repo.clone(),
                agent_client.clone(),
                nats_client.clone(),
            ),
            job_repo,
            app_repo,
            worker_repo,
            agent_client,
            nats_client,
            pool,
            router_idle_timeout_secs: 900,
        };

        service.delete_all_by_app("app-1", "user-1").await.unwrap();
    }

    #[tokio::test]
    async fn test_delete_all_by_app_returns_error_when_vm_delete_fails() {
        let job = paused_job();
        let mut job_repo = MockJobRepository::new();
        job_repo
            .expect_list_jobs()
            .returning(move |_, _, _| Ok(vec![job.clone()]));
        job_repo
            .expect_remove_jobs_by_app()
            .with(eq("app-1"))
            .times(0)
            .returning(|_| Ok(()));

        let mut agent_client = MockAgentClient::new();
        agent_client
            .expect_delete_vm()
            .with(eq("host-1"), eq("vm-1"))
            .times(1)
            .returning(|_, _| Err(DomainError::Infrastructure("boom".to_string())));

        let worker_repo = Arc::new(MockWorkerRepository::new());
        let app_repo = Arc::new(DummyAppRepo);
        let job_repo = Arc::new(job_repo);
        let agent_client = Arc::new(agent_client);
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();

        let nats_client = async_nats::connect("nats://localhost:4223").await.unwrap();
        let service = AppService {
            deployment: DeploymentService::new(
                job_repo.clone(),
                worker_repo.clone(),
                agent_client.clone(),
                nats_client.clone(),
            ),
            job_repo,
            app_repo,
            worker_repo,
            agent_client,
            nats_client,
            pool,
            router_idle_timeout_secs: 900,
        };

        let err = service
            .delete_all_by_app("app-1", "user-1")
            .await
            .expect_err("cleanup should fail");

        assert!(matches!(err, DomainError::Infrastructure(_)));
    }

    #[tokio::test]
    async fn test_resume_app_invalidates_missing_host_instead_of_calling_agent() {
        let mut job = paused_job();
        job.status = JobStatus::Paused;

        let mut job_repo = MockJobRepository::new();
        job_repo.expect_get_job().with(eq("job-1")).returning({
            let job = job.clone();
            move |_| Ok(Some(job.clone()))
        });
        job_repo
            .expect_fail_job()
            .with(
                eq("job-1"),
                mockall::predicate::function(|msg: &String| msg == "Host host-1 no longer exists"),
                mockall::predicate::function(|ts: &i64| *ts > 0),
            )
            .times(1)
            .returning(|_, _, _| Ok(()));

        let mut worker_repo = MockWorkerRepository::new();
        worker_repo
            .expect_get_worker()
            .with(eq("host-1"))
            .times(1)
            .returning(|_| Ok(None));

        let mut agent_client = MockAgentClient::new();
        agent_client.expect_resume_vm().times(0);

        let app_repo = Arc::new(DummyAppRepo);
        let job_repo = Arc::new(job_repo);
        let worker_repo = Arc::new(worker_repo);
        let agent_client = Arc::new(agent_client);
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap();

        let nats_client = async_nats::connect("nats://localhost:4223").await.unwrap();
        let service = AppService {
            deployment: DeploymentService::new(
                job_repo.clone(),
                worker_repo.clone(),
                agent_client.clone(),
                nats_client.clone(),
            ),
            job_repo,
            app_repo,
            worker_repo,
            agent_client,
            nats_client,
            pool,
            router_idle_timeout_secs: 900,
        };

        let resumed = service.resume_app("job-1", "user-1").await.unwrap();

        assert!(!resumed);
    }

    #[test]
    fn autoscaling_target_scales_down_when_usage_is_below_hysteresis_band() {
        let app = AppConfig {
            id: "app-1".to_string(),
            user_id: "user-1".to_string(),
            vpc_ipv6_prefix: "fd00::".to_string(),
            desired_replicas: 3,
            min_replicas: 1,
            max_replicas: 3,
            autoscaling_enabled: true,
            cpu_threshold: 80.0,
            mem_threshold: 80.0,
            hostname: "app.example.com".to_string(),
            last_router_traffic_at: 0,
            last_scaled_to_zero_at: 0,
        };

        let target = autoscale_next_replicas(&app, 3, 30.0, 25.0);

        assert_eq!(target, 2);
    }

    #[test]
    fn autoscaling_target_holds_size_inside_hysteresis_band() {
        let app = AppConfig {
            id: "app-1".to_string(),
            user_id: "user-1".to_string(),
            vpc_ipv6_prefix: "fd00::".to_string(),
            desired_replicas: 3,
            min_replicas: 1,
            max_replicas: 3,
            autoscaling_enabled: true,
            cpu_threshold: 80.0,
            mem_threshold: 80.0,
            hostname: "app.example.com".to_string(),
            last_router_traffic_at: 0,
            last_scaled_to_zero_at: 0,
        };

        let target = autoscale_next_replicas(&app, 3, 70.0, 70.0);

        assert_eq!(target, 3);
    }

    #[test]
    fn autoscaling_target_keeps_current_size_when_usage_matches_threshold() {
        let app = AppConfig {
            id: "app-1".to_string(),
            user_id: "user-1".to_string(),
            vpc_ipv6_prefix: "fd00::".to_string(),
            desired_replicas: 2,
            min_replicas: 1,
            max_replicas: 3,
            autoscaling_enabled: true,
            cpu_threshold: 80.0,
            mem_threshold: 80.0,
            hostname: "app.example.com".to_string(),
            last_router_traffic_at: 0,
            last_scaled_to_zero_at: 0,
        };

        let target = autoscale_next_replicas(&app, 2, 80.0, 80.0);

        assert_eq!(target, 2);
    }
}
