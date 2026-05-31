use super::orchestrator::DeploymentOrchestrator;
use crate::domain::{App, Deployment};

pub struct DeploymentPromotionWorkflow;

impl DeploymentPromotionWorkflow {
    pub fn run_zero_downtime_flow(
        state: crate::AppState,
        app: App,
        deployment: Deployment,
        inner: mikrom_proto::scheduler::DeployResponse,
        tenant_id: String,
        cleanup_on_failure: bool,
        _guard: crate::DeploymentFlowGuard,
    ) {
        use super::service;
        use std::time::Duration;
        use tracing::{debug, error, info};

        let app_id = app.id;

        tokio::spawn(async move {
            let _guard = _guard;

            let result = async {
                let old_active_id = match state.app_repo.get_app(app.id).await {
                    Ok(Some(a)) => a.active_deployment_id,
                    _ => None,
                };

                DeploymentOrchestrator::mark_previous_deployment_draining(
                    &state,
                    &app.name,
                    app.id,
                    old_active_id,
                )
                .await?;

                let mut healthy = false;
                let mut last_health_error: Option<String> = None;
                let max_attempts =
                    service::DeploymentService::zero_downtime_health_check_max_attempts();
                let health_check_timeout =
                    service::DeploymentService::zero_downtime_health_check_request_timeout();
                for attempt in 1..=max_attempts {
                    if attempt % 5 == 1 {
                        info!(
                            app = %app.name,
                            job_id = %inner.job_id,
                            attempt = attempt,
                            "Checking health for zero-downtime deployment..."
                        );
                    } else {
                        debug!(
                            app = %app.name,
                            job_id = %inner.job_id,
                            attempt = attempt,
                            "Checking health for zero-downtime deployment..."
                        );
                    }

                    let health_req = mikrom_proto::scheduler::CheckHealthRequest {
                        job_id: inner.job_id.clone(),
                        tenant_id: tenant_id.clone(),
                    };

                    match state
                        .nats
                        .with_timeout(health_check_timeout)
                        .request::<_, mikrom_proto::scheduler::CheckHealthResponse>(
                            "mikrom.scheduler.check_health",
                            health_req,
                        )
                        .await
                    {
                        Ok(resp) if resp.is_healthy => {
                            healthy = true;
                            info!(app = %app.name, "New deployment is healthy!");
                            break;
                        },
                        Ok(resp) => {
                            let message = resp.message.clone();
                            last_health_error = Some(message.clone());
                            tracing::warn!(
                                app = %app.name,
                                job_id = %inner.job_id,
                                attempt = attempt,
                                reason = %message,
                                "Health check returned unhealthy"
                            );
                            debug!(
                                app = %app.name,
                                message = %message,
                                "Health check returned unhealthy"
                            );
                        },
                        Err(e) => {
                            let message = e.to_string();
                            last_health_error = Some(message.clone());
                            tracing::warn!(
                                app = %app.name,
                                job_id = %inner.job_id,
                                attempt = attempt,
                                reason = %message,
                                "Health check request failed"
                            );
                            debug!(
                                app = %app.name,
                                error = %message,
                                "Health check request failed"
                            );
                        },
                    }
                    tokio::time::sleep(Duration::from_secs(
                        service::ZERO_DOWNTIME_HEALTH_CHECK_RETRY_DELAY_SECS,
                    ))
                    .await;
                }

                if !healthy {
                    if cleanup_on_failure {
                        error!(
                            app = %app.name,
                            reason = last_health_error.as_deref().unwrap_or("unknown"),
                            "Zero-downtime deployment failed: health check timeout. Cleaning up new VM."
                        );
                        DeploymentOrchestrator::rollback_failed_promotion(
                            &state,
                            &app.name,
                            app.id,
                            deployment.id,
                            &inner.job_id,
                            old_active_id,
                        )
                        .await?;
                    } else {
                        error!(
                            app = %app.name,
                            "Promotion failed: health check timeout. App remains in preview."
                        );
                    }
                    state.deployment_events.send(app.id).ok();
                    return Ok::<(), anyhow::Error>(());
                }

                info!(
                    app = %app.name,
                    deployment_id = %deployment.id,
                    "Promoting new deployment to active"
                );
                let (app_after_promotion, previous_active_id) =
                    DeploymentOrchestrator::promote_deployment_to_active(
                        &state,
                        app,
                        deployment.id,
                    )
                    .await?;

                // Trigger immediate ACME certification if hostname is present
                if let Some(hostname) = &app_after_promotion.hostname {
                    let state_for_acme = state.clone();
                    let hostname = hostname.clone();
                    tokio::spawn(async move {
                        if let Err(e) =
                            crate::acme::trigger_domain_certification(&state_for_acme, &hostname)
                                .await
                        {
                            tracing::error!(hostname = %hostname, error = %e, "Immediate ACME certification failed");
                        }
                    });
                }

                if let Some(old_id) = previous_active_id {
                    DeploymentOrchestrator::drain_previous_deployment_after_promotion(
                        &state,
                        &app_after_promotion.name,
                        Some(old_id),
                    )
                    .await?;
                }

                // NEW: After promotion, ensure we scale to the desired number of replicas
                info!(
                    app = %app_after_promotion.name,
                    desired = %app_after_promotion.desired_replicas,
                    "Ensuring desired replicas after promotion"
                );
                state
                    .scheduler
                    .scale_app(
                        app_after_promotion.id.to_string(),
                        app_after_promotion.desired_replicas as u32,
                        app_after_promotion.tenant_id.to_string(),
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("Post-promotion scaling failed: {}", e))?;

                Ok(())
            }
            .await;

            if let Err(e) = result {
                error!(app_id = %app_id, error = %e, "Zero-downtime deployment flow failed unexpectedly");
            }
        });
    }
}
