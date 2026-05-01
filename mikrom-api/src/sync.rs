use crate::AppState;
use crate::repositories::app_repository::UpdateDeploymentParams;
use futures::stream::{FuturesUnordered, StreamExt};
use mikrom_proto::scheduler::{AppInfo, AppStatusRequest, AppStatusResponse};
use tokio::time::{Duration, interval};
use tracing::{error, info};

pub async fn start_ip_sync_task(state: AppState) {
    let mut interval = interval(Duration::from_millis(5000)); // Poll every 5 seconds
    info!("Starting IP/Status sync background task (Optimized)");

    loop {
        interval.tick().await;

        // 1. Get all active deployments directly (status: RUNNING, STARTING)
        let deployments = match state.app_repo.list_deployments_by_user("all").await {
            Ok(deps) => deps
                .into_iter()
                .filter(|d| ["RUNNING", "STARTING"].contains(&d.status.as_str()))
                .collect::<Vec<_>>(),
            Err(_) => continue,
        };

        if deployments.is_empty() {
            continue;
        }

        let mut workers = FuturesUnordered::new();

        for dep in deployments {
            let state = state.clone();
            workers.push(async move {
                if let Some(job_id) = &dep.job_id {
                    let nats_req = AppStatusRequest {
                        job_id: job_id.clone(),
                        user_id: dep.user_id.to_string(),
                    };

                    if let Ok(inner) = state
                        .nats
                        .with_timeout(Duration::from_secs(2))
                        .request::<_, AppStatusResponse>(
                            mikrom_proto::subjects::SCHEDULER_GET_JOB,
                            nats_req,
                        )
                        .await
                    {
                        let db_status = crate::scheduler::status_name(inner.status);
                        let ip_address = inner.ip_address;

                        sync_deployment_state(&state, &dep, db_status, &ip_address).await;
                    }
                }
            });
        }

        while workers.next().await.is_some() {}
    }
}

pub async fn start_nats_job_listener(state: AppState) {
    info!("Starting instant NATS job update listener...");
    let mut sub = match state
        .nats
        .subscribe(mikrom_proto::subjects::SCHEDULER_JOB_UPDATES)
        .await
    {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to subscribe to NATS job updates: {}", e);
            return;
        },
    };

    while let Some(msg) = sub.next().await {
        use prost::Message;
        if let Ok(info) = AppInfo::decode(&msg.payload[..]) {
            let state = state.clone();
            tokio::spawn(async move {
                // Find corresponding deployment in DB
                let dep = if !info.deployment_id.is_empty() {
                    state
                        .app_repo
                        .get_deployment(
                            uuid::Uuid::parse_str(&info.deployment_id).unwrap_or_default(),
                        )
                        .await
                        .ok()
                        .flatten()
                } else {
                    state
                        .app_repo
                        .get_deployment_by_job_id(&info.job_id)
                        .await
                        .ok()
                        .flatten()
                };

                if let Some(dep) = dep {
                    let db_status = crate::scheduler::status_name(info.status);
                    sync_deployment_state(&state, &dep, db_status, "").await;
                }
            });
        }
    }
}

async fn sync_deployment_state(
    state: &AppState,
    dep: &crate::models::app::Deployment,
    db_status: &str,
    ip_address: &str,
) {
    // Prevent status downgrades (e.g., RUNNING -> PENDING due to out-of-order NATS messages)
    fn status_priority(status: &str) -> i32 {
        match status {
            "FAILED" | "CANCELLED" => 5, // Terminal states have highest priority
            "RUNNING" | "STOPPED" => 4,  // Allow transitions between running and stopped
            "SCHEDULED" => 3,
            "PENDING" => 2,
            "BUILDING" => 1,
            _ => 0,
        }
    }

    let new_priority = status_priority(db_status);
    let current_priority = status_priority(&dep.status);

    let status_changed = db_status != dep.status && new_priority >= current_priority;
    let has_new_ip = !ip_address.is_empty() && dep.ip_address.as_deref() != Some(ip_address);

    if status_changed || has_new_ip {
        let _ = state
            .app_repo
            .update_deployment(
                dep.id,
                UpdateDeploymentParams {
                    status: if status_changed {
                        Some(db_status.to_string())
                    } else {
                        None
                    },
                    job_id: dep.job_id.clone(),
                    image_tag: dep.image_tag.clone(),
                    build_id: dep.build_id.clone(),
                    ip_address: if !ip_address.is_empty() {
                        Some(ip_address.to_string())
                    } else {
                        None
                    },
                    git_commit_hash: dep.git_commit_hash.clone(),
                    git_commit_message: dep.git_commit_message.clone(),
                    git_branch: dep.git_branch.clone(),
                },
            )
            .await;
        state.deployment_events.send(dep.app_id).ok();
        let _ = state.notify_router(dep.app_id).await;
    }
}
