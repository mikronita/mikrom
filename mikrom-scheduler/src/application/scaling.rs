use super::{
    AppContext, DeploymentService, autoscale_next_replicas, update_app_config_best_effort,
};
use crate::application::lifecycle::JobLifecycleService;
use crate::domain::{AppConfig, DomainError, DomainResult, JobStatus, Worker};
use std::collections::HashMap;
use std::sync::Arc;

const AUTOSCALING_TICK_SECS: u64 = 2;

#[derive(Clone)]
pub struct ScalingService {
    ctx: Arc<AppContext>,
    deployment: DeploymentService,
    lifecycle: JobLifecycleService,
}

impl ScalingService {
    pub fn new(
        ctx: Arc<AppContext>,
        deployment: DeploymentService,
        lifecycle: JobLifecycleService,
    ) -> Self {
        Self {
            ctx,
            deployment,
            lifecycle,
        }
    }

    pub async fn scale_app(
        &self,
        app_id: &str,
        desired_replicas: u32,
        user_id: &str,
    ) -> DomainResult<()> {
        let telemetry = self.ctx.telemetry.clone();
        telemetry
            .observe_result("scaling", "scale_app", async {
        let jobs = self
            .ctx
            .job_repo
            .list_jobs(Some(user_id), None, None)
            .await?;
        let active_jobs: Vec<_> = jobs
            .into_iter()
            .filter(|j| {
                j.app_id.as_ref() == app_id
                    && matches!(
                        j.status,
                        JobStatus::Pending | JobStatus::Scheduled | JobStatus::Running
                    )
            })
            .collect();
        let paused_jobs: Vec<_> = self
            .ctx
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
                        match self.lifecycle.resume_app(&job.job_id, user_id).await {
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

            let app_config = self.ctx.app_repo.get_app_config(app_id).await?;
            let vpc_prefix = app_config.map(|c| c.vpc_ipv6_prefix).unwrap_or_default();

            let mut template_job = active_jobs
                .first()
                .cloned()
                .or_else(|| paused_jobs.first().cloned());

            if template_job.is_none() {
                let mut all_jobs = self
                    .ctx
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
                let app_id = template_job.app_id.to_string();
                let app_name = template_job.app_name.clone();
                let image = template_job.image.clone();
                let user_id = template_job.user_id.to_string();
                let deployment_id = template_job
                    .deployment_id
                    .clone()
                    .unwrap_or_default()
                    .to_string();
                let vpc_prefix = vpc_prefix.clone();
                let config = template_job.config.clone();

                deployment_futures.push(async move {
                    deployment
                        .deploy_app(crate::application::deployment::DeployAppParams {
                            app_id,
                            app_name,
                            image,
                            user_id,
                            deployment_id,
                            vpc_ipv6_prefix: vpc_prefix,
                            config,
                            strategy: crate::domain::worker::SchedulingStrategy::LeastLoaded,
                        })
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
                        self.lifecycle.pause_app(&job.job_id, user_id).await?;
                    } else {
                        self.lifecycle.delete_app(&job.job_id, user_id).await?;
                    }
                } else {
                    self.lifecycle.delete_app(&job.job_id, user_id).await?;
                }
            }
        }

        Ok(())
            })
            .await
    }

    pub async fn start_autoscaler(&self) {
        tracing::info!("Starting background autoscaler loop");
        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(AUTOSCALING_TICK_SECS));

        loop {
            interval.tick().await;
            if let Err(e) = self.reconcile_apps().await {
                tracing::error!("App reconciliation failed: {}", e);
            }
        }
    }

    pub async fn reconcile_apps(&self) -> DomainResult<()> {
        let telemetry = self.ctx.telemetry.clone();
        telemetry
            .observe_result("scaling", "reconcile_apps", async {
                let apps = self
                    .ctx
                    .app_repo
                    .list_all_apps()
                    .await
                    .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

                if apps.is_empty() {
                    return Ok(());
                }

                let all_jobs = self
                    .ctx
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

                let workers = self
                    .ctx
                    .worker_repo
                    .list_workers()
                    .await
                    .map_err(|e| DomainError::Infrastructure(e.to_string()))?;
                let worker_map: HashMap<String, Worker> = workers
                    .into_iter()
                    .map(|w| (w.host_id.to_string(), w))
                    .collect();

                let mut app_running_counts: HashMap<String, u32> = HashMap::new();
                let mut app_metrics: HashMap<String, (f32, f32)> = HashMap::new();

                for job in all_jobs {
                    let count = app_running_counts
                        .entry(job.app_id.to_string())
                        .or_insert(0);
                    *count += 1;

                    let vm_metrics = job
                        .host_id
                        .as_ref()
                        .and_then(|h| job.vm_id.as_ref().map(|v| (h, v)))
                        .and_then(|(h, _v)| worker_map.get(h.as_ref()))
                        .and_then(|w| w.metrics.as_ref())
                        .and_then(|m| m.vms.get(job.vm_id.as_ref().unwrap().as_ref()));

                    if let Some(vm_metrics) = vm_metrics {
                        let entry = app_metrics
                            .entry(job.app_id.to_string())
                            .or_insert((0.0, 0.0));
                        entry.0 += vm_metrics.cpu_usage;
                        entry.1 += (vm_metrics.ram_used_bytes as f32
                            / job.config.memory_mib as f32
                            / 1024.0
                            / 1024.0)
                            * 100.0;
                    }
                }

                let now = chrono::Utc::now().timestamp();

                for mut app in apps {
                    let current_count = *app_running_counts.get(app.id.as_ref()).unwrap_or(&0);
                    let metrics = app_metrics.get(app.id.as_ref()).copied();

                    self.reconcile_app_state_initialization(&mut app, current_count, now)
                        .await?;

                    if self
                        .reconcile_idle_timeout(&mut app, current_count, now)
                        .await?
                    {
                        continue;
                    }

                    if self
                        .reconcile_restore_from_zero(&mut app, current_count, now)
                        .await?
                    {
                        continue;
                    }

                    if app.autoscaling_enabled {
                        self.reconcile_autoscaling(&mut app, current_count, metrics)
                            .await?;
                    } else {
                        self.reconcile_manual_scaling(&app, current_count).await?;
                    }
                }

                Ok(())
            })
            .await
    }

    async fn reconcile_app_state_initialization(
        &self,
        app: &mut AppConfig,
        current_count: u32,
        now: i64,
    ) -> DomainResult<()> {
        if current_count > 0
            && app.min_replicas == 0
            && app.desired_replicas > 0
            && app.last_router_traffic_at == 0
        {
            app.last_router_traffic_at = now;
            update_app_config_best_effort(
                &self.ctx.app_repo,
                app.clone(),
                "reconcile-init-router-traffic",
            )
            .await;
        }

        if current_count > 0 && app.restore_retry_after_at > 0 {
            app.restore_retry_after_at = 0;
            update_app_config_best_effort(
                &self.ctx.app_repo,
                app.clone(),
                "reconcile-init-clear-backoff",
            )
            .await;
        }

        Ok(())
    }

    async fn reconcile_idle_timeout(
        &self,
        app: &mut AppConfig,
        current_count: u32,
        now: i64,
    ) -> DomainResult<bool> {
        let router_idle = app.min_replicas == 0
            && app.last_router_traffic_at > 0
            && now - app.last_router_traffic_at >= self.ctx.runtime.router_idle_timeout_secs;

        if current_count > 0 && router_idle {
            tracing::info!(
                event = "scale_to_zero",
                app_id = %app.id,
                last_router_traffic_at = %app.last_router_traffic_at,
                timeout_secs = self.ctx.runtime.router_idle_timeout_secs,
                "No router traffic for configured idle timeout; scaling app to zero"
            );

            app.last_scaled_to_zero_at = now;
            update_app_config_best_effort(
                &self.ctx.app_repo,
                app.clone(),
                "reconcile-idle-scale-to-zero",
            )
            .await;

            if let Err(e) = self.scale_app(&app.id, 0, &app.user_id).await {
                tracing::error!(
                    app_id = %app.id,
                    error = %e,
                    "Failed to scale app to zero after router inactivity"
                );
            }

            return Ok(true);
        }

        Ok(false)
    }

    async fn reconcile_restore_from_zero(
        &self,
        app: &mut AppConfig,
        current_count: u32,
        now: i64,
    ) -> DomainResult<bool> {
        if current_count > 0 {
            return Ok(false);
        }

        if app.last_scaled_to_zero_at == 0 && app.last_router_traffic_at == 0 {
            app.last_scaled_to_zero_at = now;
            update_app_config_best_effort(
                &self.ctx.app_repo,
                app.clone(),
                "reconcile-init-saw-zero-state",
            )
            .await;
            return Ok(true);
        }

        let should_restore_from_zero = app.min_replicas == 0
            && app.desired_replicas > 0
            && app.last_router_traffic_at > app.last_scaled_to_zero_at;

        if should_restore_from_zero {
            let restore_retry_blocked =
                app.restore_retry_after_at > 0 && now < app.restore_retry_after_at;

            if restore_retry_blocked {
                tracing::warn!(
                    app_id = %app.id,
                    retry_after = %app.restore_retry_after_at,
                    "Skipping router-triggered restore while backoff is active"
                );
                return Ok(true);
            }

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
            } else {
                app.last_scaled_to_zero_at = now;
                app.restore_retry_after_at = 0;
                update_app_config_best_effort(
                    &self.ctx.app_repo,
                    app.clone(),
                    "reconcile-restore-from-zero",
                )
                .await;
            }
            return Ok(true);
        }

        if app.min_replicas > 0 {
            tracing::info!(app_id = %app.id, "Scaling up to min_replicas");
            if let Err(e) = self
                .scale_app(&app.id, app.min_replicas, &app.user_id)
                .await
            {
                tracing::error!("Failed to scale app {} to min: {}", app.id, e);
            } else {
                app.desired_replicas = app.min_replicas;
                update_app_config_best_effort(
                    &self.ctx.app_repo,
                    app.clone(),
                    "reconcile-scale-to-min",
                )
                .await;
            }
            return Ok(true);
        }

        Ok(false)
    }

    async fn reconcile_autoscaling(
        &self,
        app: &mut AppConfig,
        current_count: u32,
        metrics: Option<(f32, f32)>,
    ) -> DomainResult<()> {
        if let Some((total_cpu, total_mem)) = metrics {
            let avg_cpu = total_cpu / (current_count as f32);
            let avg_mem = total_mem / (current_count as f32);

            tracing::debug!(
                app_id = %app.id,
                avg_cpu = %avg_cpu,
                avg_mem = %avg_mem,
                count = %current_count,
                "Evaluating autoscaling"
            );

            let desired = autoscale_next_replicas(app, current_count, avg_cpu, avg_mem);

            if desired != current_count {
                if desired > current_count {
                    tracing::info!(
                        app_id = %app.id,
                        avg_cpu = %avg_cpu,
                        avg_mem = %avg_mem,
                        "Scale up triggered (auto)"
                    );
                } else {
                    tracing::info!(
                        app_id = %app.id,
                        avg_cpu = %avg_cpu,
                        avg_mem = %avg_mem,
                        "Scale down triggered (auto)"
                    );
                }

                if let Err(e) = self.scale_app(&app.id, desired, &app.user_id).await {
                    tracing::error!(
                        app_id = %app.id,
                        desired = %desired,
                        error = %e,
                        "Failed to reconcile autoscaling"
                    );
                } else {
                    app.desired_replicas = desired;
                    update_app_config_best_effort(
                        &self.ctx.app_repo,
                        app.clone(),
                        "reconcile-autoscaling-update",
                    )
                    .await;
                }
            }
        }
        Ok(())
    }

    async fn reconcile_manual_scaling(
        &self,
        app: &AppConfig,
        current_count: u32,
    ) -> DomainResult<()> {
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
        Ok(())
    }
}
