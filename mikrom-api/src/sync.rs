use crate::AppState;
use futures::stream::{FuturesUnordered, StreamExt};
use tokio::time::{Duration, interval};
use tracing::{debug, error, info};

pub async fn start_ip_sync_task(state: AppState) {
    let mut interval = interval(Duration::from_secs(5));
    info!("Starting IP sync background task");

    loop {
        interval.tick().await;

        // 1. Get all applications
        let apps = match state.app_repo.list_apps_by_user("all").await {
            Ok(apps) => apps,
            Err(e) => {
                error!("Failed to list apps for sync: {}", e);
                continue;
            },
        };

        // 2. Try to get scheduler client
        let client = match state.get_scheduler_client().await {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to get scheduler client for sync: {}", e);
                continue;
            },
        };

        let mut workers = FuturesUnordered::new();

        for app in apps {
            let app_id = app.id;
            let app_name = app.name.clone();
            let state = state.clone();
            let mut client = client.clone();

            workers.push(async move {
                let deployments = state
                    .app_repo
                    .list_deployments_by_app(app_id)
                    .await
                    .unwrap_or_default();

                let running_deps: Vec<_> = deployments.into_iter().filter(|d| d.status == "RUNNING").collect();

                for dep in running_deps {
                    if let Some(job_id) = &dep.job_id {
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
                                    info!(app = %app_name, ip = %inner.ip_address, "Syncing real IP from scheduler to DB");
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
                                    state.deployment_events.send(dep.app_id).ok();
                                }
                            },
                            Err(status) if status.code() == tonic::Code::NotFound => {
                                info!(app = %app_name, job_id = %job_id, "Job not found in scheduler, marking as STOPPED in DB");
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
                                state.deployment_events.send(dep.app_id).ok();
                            },
                            Err(e) => {
                                debug!(error = %e, "Failed to get app status from scheduler");
                            },
                        }
                    }
                }
            });
        }

        while workers.next().await.is_some() {}
    }
}
