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

        // 1. Get all active deployments directly (non-terminal states)
        let deployments = match state.app_repo.list_deployments_by_user(None).await {
            Ok(deps) => deps
                .into_iter()
                .filter(|d| !["STOPPED", "FAILED", "CANCELLED"].contains(&d.status.as_str()))
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
                        let ipv6_address = inner.ipv6_address;

                        sync_deployment_state(&state, &dep, db_status, &ipv6_address).await;
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
                    sync_deployment_state(&state, &dep, db_status, &info.ipv6_address).await;
                }
            });
        }
    }
}

async fn sync_deployment_state(
    state: &AppState,
    dep: &crate::models::app::Deployment,
    db_status: &str,
    ipv6_address: &str,
) {
    let active_app = state.app_repo.get_app(dep.app_id).await.ok().flatten();
    let active_deployment_id = active_app.as_ref().and_then(|app| app.active_deployment_id);

    let status_changed =
        should_apply_cluster_status(dep.status.as_str(), db_status, active_deployment_id, dep.id);
    let has_new_ipv6 =
        !ipv6_address.is_empty() && dep.ipv6_address.as_deref() != Some(ipv6_address);

    let deployment_changed = status_changed || has_new_ipv6;

    if deployment_changed {
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
                    ipv6_address: if !ipv6_address.is_empty() {
                        Some(ipv6_address.to_string())
                    } else {
                        dep.ipv6_address.clone()
                    },
                    git_commit_hash: dep.git_commit_hash.clone(),
                    git_commit_message: dep.git_commit_message.clone(),
                    git_branch: dep.git_branch.clone(),
                },
            )
            .await;
        state.deployment_events.send(dep.app_id).ok();
    }
}

fn should_apply_cluster_status(
    current_status: &str,
    incoming_status: &str,
    active_deployment_id: Option<uuid::Uuid>,
    dep_id: uuid::Uuid,
) -> bool {
    if current_status == incoming_status {
        return false;
    }

    match (current_status, incoming_status) {
        ("DRAINING", "RUNNING") => false,
        ("DRAINING", "PENDING") | ("DRAINING", "SCHEDULED") | ("DRAINING", "BUILDING") => false,
        ("DRAINING", "PAUSED" | "STOPPED") => true,
        ("DRAINING", "FAILED" | "CANCELLED") => false,
        ("PAUSED" | "STOPPED", "RUNNING") => active_deployment_id == Some(dep_id),
        ("FAILED" | "CANCELLED", "RUNNING") => false,
        _ => status_priority(incoming_status) >= status_priority(current_status),
    }
}

fn status_priority(status: &str) -> i32 {
    match status {
        "FAILED" | "CANCELLED" => 5,
        "RUNNING" | "DRAINING" | "PAUSED" | "STOPPED" => 4,
        "SCHEDULED" => 3,
        "PENDING" => 2,
        "BUILDING" => 1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn draining_deployment_cannot_be_revived_by_running_heartbeat() {
        let dep_id = Uuid::new_v4();

        assert!(!should_apply_cluster_status(
            "DRAINING",
            "RUNNING",
            Some(dep_id),
            dep_id
        ));

        assert!(!should_apply_cluster_status(
            "DRAINING",
            "RUNNING",
            Some(Uuid::new_v4()),
            dep_id
        ));
    }

    #[test]
    fn paused_or_stopped_only_accept_running_when_it_is_the_active_deployment() {
        let dep_id = Uuid::new_v4();

        assert!(should_apply_cluster_status(
            "PAUSED",
            "RUNNING",
            Some(dep_id),
            dep_id
        ));

        assert!(!should_apply_cluster_status(
            "PAUSED",
            "RUNNING",
            Some(Uuid::new_v4()),
            dep_id
        ));

        assert!(should_apply_cluster_status(
            "STOPPED",
            "RUNNING",
            Some(dep_id),
            dep_id
        ));
    }

    #[test]
    fn draining_never_transitions_to_failed_from_late_cluster_events() {
        let dep_id = Uuid::new_v4();

        assert!(!should_apply_cluster_status(
            "DRAINING",
            "FAILED",
            Some(dep_id),
            dep_id
        ));
        assert!(!should_apply_cluster_status(
            "DRAINING",
            "CANCELLED",
            Some(dep_id),
            dep_id
        ));
    }
}
