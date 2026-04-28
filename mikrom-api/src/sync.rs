use crate::AppState;
use futures::stream::{FuturesUnordered, StreamExt};
use mikrom_proto::scheduler::{AppInfo, AppStatusRequest, AppStatusResponse};
use prost::Message;
use tokio::time::{Duration, interval};
use tracing::{error, info};

pub async fn start_ip_sync_task(state: AppState) {
    let mut interval = interval(Duration::from_millis(1000));
    info!("Starting IP/Status sync background task (NATS/Protobuf)");

    loop {
        interval.tick().await;

        // 1. Get all applications
        let apps = match state.app_repo.list_apps_by_user("all").await {
            Ok(apps) => apps,
            Err(_) => continue,
        };

        let mut workers = FuturesUnordered::new();

        for app in apps {
            let app_id = app.id;
            let state = state.clone();

            workers.push(async move {
                let deployments = state
                    .app_repo
                    .list_deployments_by_app(app_id)
                    .await
                    .unwrap_or_default();

                let active_deps: Vec<_> = deployments
                    .into_iter()
                    .filter(|d| {
                        ["RUNNING", "STOPPED", "STARTING", "SCHEDULED", "BUILDING"]
                            .contains(&d.status.as_str())
                    })
                    .collect();

                for dep in active_deps {
                    if let Some(job_id) = &dep.job_id {
                        let nats_req = AppStatusRequest {
                            job_id: job_id.clone(),
                            user_id: dep.user_id.to_string(),
                        };

                        let mut buf = Vec::new();
                        if nats_req.encode(&mut buf).is_err() {
                            continue;
                        }

                        let status_res = state
                            .nats_client
                            .request("mikrom.scheduler.get_job", buf.into())
                            .await;

                        if let Some(inner) = status_res
                            .ok()
                            .and_then(|r| AppStatusResponse::decode(&r.payload[..]).ok())
                        {
                            let db_status = crate::scheduler::status_name(inner.status);
                            let ip_address = inner.ip_address;

                            sync_deployment_state(&state, &dep, db_status, &ip_address).await;
                        }
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
        .nats_client
        .subscribe("mikrom.scheduler.job_updates")
        .await
    {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to subscribe to NATS job updates: {}", e);
            return;
        },
    };

    while let Some(msg) = sub.next().await {
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
            .update_deployment_status(
                dep.id,
                if status_changed {
                    db_status
                } else {
                    &dep.status
                },
                dep.job_id.clone(),
                dep.image_tag.clone(),
                dep.build_id.clone(),
                if !ip_address.is_empty() {
                    Some(ip_address.to_string())
                } else {
                    dep.ip_address.clone()
                },
                dep.git_commit_hash.clone(),
                dep.git_commit_message.clone(),
                dep.git_branch.clone(),
            )
            .await;
        state.deployment_events.send(dep.app_id).ok();
    }
}
