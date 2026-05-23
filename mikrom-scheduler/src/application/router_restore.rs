use crate::application::{AppContext, ScalingService, update_app_config_best_effort};
use crate::domain::{AppConfig, DomainResult, JobStatus};
use mikrom_proto::router::RouterTrafficEvent;
use std::sync::Arc;

#[derive(Clone)]
pub struct RouterRestoreService {
    ctx: Arc<AppContext>,
    scaling: ScalingService,
}

impl RouterRestoreService {
    pub fn new(ctx: Arc<AppContext>, scaling: ScalingService) -> Self {
        Self { ctx, scaling }
    }

    pub async fn process_router_traffic(&self, event: RouterTrafficEvent) -> DomainResult<()> {
        let telemetry = self.ctx.telemetry.clone();
        telemetry
            .observe_result("event", "router_traffic", async {
                tracing::info!(
                    hostname = %event.hostname,
                    router_id = %event.router_id,
                    timestamp = %event.timestamp,
                    "Received router traffic event"
                );

                let Some(mut app) = self
                    .ctx
                    .app_repo
                    .get_app_config_by_hostname(&event.hostname)
                    .await?
                else {
                    return Ok(());
                };

                let timestamp = if event.timestamp > 0 {
                    event.timestamp
                } else {
                    chrono::Utc::now().timestamp()
                };

                app.last_router_traffic_at = timestamp;
                self.ctx.app_repo.update_app_config(app.clone()).await?;

                let current_count = self
                    .ctx
                    .job_repo
                    .list_jobs(Some(&app.user_id), Some(&app.id), None)
                    .await?
                    .into_iter()
                    .filter(|job| {
                        matches!(
                            job.status,
                            JobStatus::Pending | JobStatus::Scheduled | JobStatus::Running
                        )
                    })
                    .count() as u32;

                if current_count == 0 && app.desired_replicas > 0 {
                    let restore_retry_blocked =
                        app.restore_retry_after_at > 0 && timestamp < app.restore_retry_after_at;

                    if restore_retry_blocked {
                        tracing::warn!(
                            app_id = %app.id,
                            hostname = %event.hostname,
                            retry_after = %app.restore_retry_after_at,
                            "Skipping router-triggered restore while backoff is active"
                        );
                        return Ok(());
                    }

                    tracing::info!(
                        event = "restore_from_router_traffic",
                        app_id = %app.id,
                        hostname = %event.hostname,
                        desired = %app.desired_replicas,
                        "Router traffic arrived for a scaled-to-zero app; restoring replicas"
                    );

                    if let Err(e) = self
                        .scaling
                        .scale_app(&app.id, app.desired_replicas, &app.user_id)
                        .await
                    {
                        tracing::error!(
                            app_id = %app.id,
                            hostname = %event.hostname,
                            error = %e,
                            "Failed to restore app after router traffic"
                        );
                    } else {
                        update_app_config_best_effort(
                            &self.ctx.app_repo,
                            AppConfig {
                                last_scaled_to_zero_at: timestamp,
                                restore_retry_after_at: 0,
                                ..app.clone()
                            },
                            "router-traffic-restore-update",
                        )
                        .await;
                        tracing::info!(
                            event = "restore_from_router_traffic_completed",
                            app_id = %app.id,
                            hostname = %event.hostname,
                            desired = %app.desired_replicas,
                            "App restored after router traffic"
                        );
                    }
                }

                Ok(())
            })
            .await
    }
}
