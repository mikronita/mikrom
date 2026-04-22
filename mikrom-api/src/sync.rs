use crate::AppState;
use tokio::time::{Duration, interval};
use tracing::info;

pub async fn start_ip_sync_task(state: AppState) {
    let mut interval = interval(Duration::from_secs(5));
    info!("Starting IP sync background task");

    loop {
        interval.tick().await;

        // 1. Get all applications
        let apps = state
            .app_repo
            .list_apps_by_user("all")
            .await
            .unwrap_or_default();

        for app in apps {
            let deployments = state
                .app_repo
                .list_deployments_by_app(app.id)
                .await
                .map(deployments_to_sync)
                .unwrap_or_default();

            for dep in deployments {
                if let (Some(job_id), Ok(channel)) = (
                    &dep.job_id,
                    crate::scheduler::connect(&state.scheduler_config).await,
                ) {
                    let mut client = mikrom_proto::scheduler::SchedulerServiceClient::new(channel);
                    let status_res = client
                        .get_app_status(mikrom_proto::scheduler::AppStatusRequest {
                            job_id: job_id.clone(),
                            user_id: dep.user_id.to_string(),
                        })
                        .await;

                    match status_res {
                        Ok(resp) => {
                            let inner = resp.into_inner();
                            let has_new_ip = !inner.ip_address.is_empty()
                                && dep.ip_address.as_deref() != Some(&inner.ip_address);

                            if has_new_ip {
                                info!(app = %app.name, ip = %inner.ip_address, "Syncing real IP from scheduler to DB");
                                let _ = state
                                    .app_repo
                                    .update_deployment_status(
                                        dep.id,
                                        "RUNNING",
                                        Some(job_id.clone()),
                                        dep.image_tag.clone(),
                                        dep.build_id.clone(),
                                        Some(inner.ip_address),
                                    )
                                    .await;
                            }
                        }
                        Err(status) if status.code() == tonic::Code::NotFound => {
                            info!(app = %app.name, job_id = %job_id, "Job not found in scheduler, marking as STOPPED in DB");
                            let _ = state
                                .app_repo
                                .update_deployment_status(
                                    dep.id,
                                    "STOPPED",
                                    Some(job_id.clone()),
                                    dep.image_tag.clone(),
                                    dep.build_id.clone(),
                                    None,
                                )
                                .await;
                        }
                        Err(e) => {
                            // Other errors (like connection) are ignored to retry later
                            tracing::debug!(error = %e, "Failed to get app status from scheduler");
                        }
                    }
                }
            }
        }
    }
}

fn deployments_to_sync(
    deps: Vec<crate::models::app::Deployment>,
) -> Vec<crate::models::app::Deployment> {
    deps.into_iter().filter(|d| d.status == "RUNNING").collect()
}
