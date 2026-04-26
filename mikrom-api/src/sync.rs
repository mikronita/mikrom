use crate::AppState;
use futures::stream::{FuturesUnordered, StreamExt};
use tokio::time::{Duration, interval};
use tracing::{debug, error, info};

pub async fn start_ip_sync_task(state: AppState) {
    let mut interval = interval(Duration::from_secs(2));
    info!("Starting IP/Status sync background task");

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

                let active_deps: Vec<_> = deployments.into_iter()
                    .filter(|d| ["RUNNING", "STOPPED", "STARTING", "SCHEDULED"].contains(&d.status.as_str()))
                    .collect();

                for dep in active_deps {
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
                                let proto_status = mikrom_proto::scheduler::DeployStatus::try_from(inner.status).unwrap_or(mikrom_proto::scheduler::DeployStatus::Unspecified);
                                let db_status = map_deploy_status(proto_status);

                                let status_changed = db_status != dep.status;
                                let has_new_ip = !inner.ip_address.is_empty()
                                    && dep.ip_address.as_deref() != Some(&inner.ip_address);

                                if status_changed || has_new_ip {
                                    if status_changed {
                                        info!(app = %app_name, job_id = %job_id, from = %dep.status, to = %db_status, "Syncing status from scheduler to DB");
                                    }
                                    if has_new_ip {
                                        info!(app = %app_name, ip = %inner.ip_address, "Syncing real IP from scheduler to DB");
                                    }

                                    let _ = state
                                        .app_repo
                                        .update_deployment_status(
                                            dep.id,
                                            &db_status,
                                            Some(job_id.clone()),
                                            dep.image_tag.clone(),
                                            dep.build_id.clone(),
                                            if !inner.ip_address.is_empty() { Some(inner.ip_address) } else { dep.ip_address.clone() },
                                            dep.git_commit_hash.clone(),
                                            dep.git_commit_message.clone(),
                                            dep.git_branch.clone(),
                                        )
                                        .await;
                                    state.deployment_events.send(dep.app_id).ok();
                                }
                            },
                            Err(status) if status.code() == tonic::Code::NotFound => {
                                info!(app = %app_name, job_id = %job_id, "Job not found in scheduler (hibernated), marking as STOPPED in DB");
                                let _ = state
                                    .app_repo
                                    .update_deployment_status(
                                        dep.id,
                                        "STOPPED",
                                        Some(job_id.clone()),
                                        dep.image_tag.clone(),
                                        dep.build_id.clone(),
                                        None,
                                        dep.git_commit_hash.clone(),
                                        dep.git_commit_message.clone(),
                                        dep.git_branch.clone(),
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

fn map_deploy_status(status: mikrom_proto::scheduler::DeployStatus) -> String {
    match status {
        mikrom_proto::scheduler::DeployStatus::Running => "RUNNING".to_string(),
        mikrom_proto::scheduler::DeployStatus::Paused => "STOPPED".to_string(),
        mikrom_proto::scheduler::DeployStatus::Failed => "FAILED".to_string(),
        mikrom_proto::scheduler::DeployStatus::Cancelled => "CANCELLED".to_string(),
        mikrom_proto::scheduler::DeployStatus::Scheduled => "SCHEDULED".to_string(),
        mikrom_proto::scheduler::DeployStatus::Pending => "PENDING".to_string(),
        _ => "UNKNOWN".to_string(),
    }
}
