use crate::application::{
    AppContext, publish_job_update_best_effort, update_app_config_best_effort,
};
use crate::domain::{DomainResult, HostMetrics, Job, JobStatus, Worker};
use mikrom_proto::agent::VmFailureEvent;
use mikrom_proto::scheduler::{RouterHeartbeat, WorkerHeartbeat};
use std::sync::Arc;

pub struct HeartbeatService {
    ctx: Arc<AppContext>,
}

impl HeartbeatService {
    pub fn new(ctx: Arc<AppContext>) -> Self {
        Self { ctx }
    }

    pub async fn process_worker_heartbeat(&self, heartbeat: WorkerHeartbeat) -> DomainResult<()> {
        let telemetry = self.ctx.telemetry.clone();
        telemetry
            .observe_result("event", "worker_heartbeat", async {
                use mikrom_proto::scheduler::VmStatus as ProtoVmStatus;
                let now = chrono::Utc::now().timestamp();
                let mut first_error: Option<crate::domain::DomainError> = None;

                let worker = Worker {
                    host_id: crate::domain::HostId::from(heartbeat.host_id.clone()),
                    hostname: heartbeat.hostname.clone(),
                    advertise_address: heartbeat.advertise_address.clone(),
                    wireguard_pubkey: Some(heartbeat.wireguard_pubkey.clone()),
                    wireguard_ip: Some(heartbeat.wireguard_ip.clone()),
                    wireguard_port: Some(heartbeat.wireguard_port),
                    metrics: None,
                    registered_at: now,
                    last_heartbeat: now,
                    status: crate::domain::WorkerStatus::Online,
                    supported_hypervisors: heartbeat
                        .supported_hypervisors
                        .iter()
                        .filter_map(|&v| crate::domain::job::HypervisorType::from_i32(v))
                        .collect(),
                };

                self.ctx.worker_repo.register(worker).await?;

                if let Some(metrics) = heartbeat.metrics {
                    let running_jobs_by_vm = self
                        .ctx
                        .job_repo
                        .list_jobs(None, None, Some(JobStatus::Running))
                        .await?
                        .into_iter()
                        .filter(|job| job.host_id.as_deref() == Some(&heartbeat.host_id))
                        .filter_map(|job| job.vm_id.clone().map(|vm_id| (vm_id.to_string(), job)))
                        .collect::<std::collections::HashMap<_, _>>();

                    for (vm_id, vm_metrics) in &metrics.vms {
                        if vm_metrics.status != ProtoVmStatus::Failed as i32 {
                            continue;
                        }

                        let Some(job) = running_jobs_by_vm.get(vm_id.as_str()) else {
                            continue;
                        };

                        let message = if vm_metrics.error_message.is_empty() {
                            "VM startup failed".to_string()
                        } else {
                            vm_metrics.error_message.clone()
                        };

                        tracing::error!(
                            job_id = %job.job_id,
                            vm_id = %vm_id,
                            host_id = %heartbeat.host_id,
                            error = %message,
                            "Detected failed VM in worker heartbeat"
                        );

                        if let Err(e) = self
                            .ctx
                            .job_repo
                            .fail_job(&job.job_id, message.clone(), now)
                            .await
                        {
                            tracing::error!(
                                job_id = %job.job_id,
                                vm_id = %vm_id,
                                host_id = %heartbeat.host_id,
                                error = %e,
                                "Failed to persist failed VM state from worker heartbeat"
                            );
                            if first_error.is_none() {
                                first_error = Some(e);
                            }
                            continue;
                        }

                        let mut updated_job = job.clone();
                        updated_job.status = JobStatus::Failed;
                        updated_job.stopped_at = Some(now);
                        updated_job.error_message = Some(message);

                        match self.ctx.app_repo.get_app_config(&updated_job.app_id).await {
                            Ok(Some(mut app)) => {
                                let retry_after = now + self.ctx.runtime.restore_retry_backoff_secs;
                                app.restore_retry_after_at = retry_after;
                                update_app_config_best_effort(
                                    &self.ctx.app_repo,
                                    app,
                                    "worker-heartbeat-restore-backoff",
                                )
                                .await;
                            },
                            Ok(None) => {},
                            Err(e) => {
                                tracing::warn!(
                                    app_id = %updated_job.app_id,
                                    error = %e,
                                    "Failed to load app config while handling worker heartbeat VM failure"
                                );
                            },
                        }

                        publish_job_update_best_effort(
                            &self.ctx.nats_client,
                            &updated_job,
                            "worker-heartbeat-failed-vm",
                        )
                        .await;
                    }

                    for (vm_id, vm_metrics) in &metrics.vms {
                        if vm_metrics.status == ProtoVmStatus::Failed as i32 {
                            continue;
                        }

                        let Some(job) = running_jobs_by_vm.get(vm_id.as_str()) else {
                            continue;
                        };

                        self.publish_metrics_event(&job.app_id, vm_id, job, vm_metrics)
                            .await;
                    }

                    let host_metrics = HostMetrics {
                        cpu_usage: metrics.cpu_usage,
                        ram_used_bytes: metrics.ram_used_bytes,
                        ram_total_bytes: metrics.ram_total_bytes,
                        disk_used_bytes: metrics.disk_used_bytes,
                        disk_total_bytes: metrics.disk_total_bytes,
                        apps_count: metrics.apps_count,
                        load_avg_1: metrics.load_avg_1,
                        load_avg_5: metrics.load_avg_5,
                        load_avg_15: metrics.load_avg_15,
                        timestamp: metrics.timestamp,
                        vms: metrics
                            .vms
                            .into_iter()
                            .map(|(k, v)| {
                                (
                                    k,
                                    crate::domain::VmMetrics {
                                        cpu_usage: v.cpu_usage,
                                        ram_used_bytes: v.ram_used_bytes,
                                        tx_bytes: v.tx_bytes,
                                        rx_bytes: v.rx_bytes,
                                    },
                                )
                            })
                            .collect(),
                    };
                    self.ctx
                        .worker_repo
                        .update_metrics(&heartbeat.host_id, host_metrics)
                        .await?;
                }

                if let Some(err) = first_error {
                    return Err(err);
                }

                Ok(())
            })
            .await
    }

    async fn publish_metrics_event(
        &self,
        app_id: &str,
        vm_id: &str,
        job: &Job,
        vm_metrics: &mikrom_proto::scheduler::VmMetrics,
    ) {
        let event = serde_json::json!({
            "app_id": app_id,
            "job_id": job.job_id.to_string(),
            "deployment_id": job.deployment_id.clone().unwrap_or_default().to_string(),
            "vm_id": vm_id,
            "cpu_usage": vm_metrics.cpu_usage,
            "ram_used_bytes": vm_metrics.ram_used_bytes,
            "tx_bytes": vm_metrics.tx_bytes,
            "rx_bytes": vm_metrics.rx_bytes,
            "status": "RUNNING",
            "ipv6_address": job.config.ipv6_address,
        });

        let subject = format!("mikrom.metrics.{}.{}", app_id, vm_id);
        let started = std::time::Instant::now();
        let result = self
            .ctx
            .nats_client
            .publish(subject.clone(), event.to_string().into())
            .await;

        self.ctx.telemetry.record(
            "event",
            "vm_metrics_publish",
            started.elapsed(),
            result.is_ok(),
        );

        if let Err(e) = result {
            tracing::warn!(
                app_id = %app_id,
                vm_id = %vm_id,
                error = %e,
                "Failed to publish VM metrics event"
            );
        }
    }

    pub async fn process_router_heartbeat(&self, heartbeat: RouterHeartbeat) -> DomainResult<()> {
        let telemetry = self.ctx.telemetry.clone();
        telemetry
            .observe_result("event", "router_heartbeat", async {
                let now = chrono::Utc::now().timestamp();
                let worker = Worker {
                    host_id: crate::domain::HostId::from(heartbeat.host_id.clone()),
                    hostname: heartbeat.hostname.clone(),
                    advertise_address: heartbeat.advertise_address.clone(),
                    wireguard_pubkey: Some(heartbeat.wireguard_pubkey.clone()),
                    wireguard_ip: Some(heartbeat.wireguard_ip.clone()),
                    wireguard_port: Some(heartbeat.wireguard_port),
                    metrics: None,
                    registered_at: now,
                    last_heartbeat: now,
                    status: crate::domain::WorkerStatus::Online,
                    supported_hypervisors: vec![],
                };

                self.ctx.worker_repo.register(worker).await?;
                Ok(())
            })
            .await
    }

    pub async fn process_vm_failure(&self, event: VmFailureEvent) -> DomainResult<()> {
        let telemetry = self.ctx.telemetry.clone();
        telemetry
            .observe_result("event", "vm_failure", async {
                let Some(job) = self.ctx.job_repo.find_job_by_vm_id(&event.vm_id).await? else {
                    tracing::warn!(vm_id = %event.vm_id, "VM failure event received for unknown job");
                    return Ok(());
                };

                if matches!(
                    job.status,
                    JobStatus::Failed | JobStatus::Cancelled | JobStatus::Stopped
                ) {
                    tracing::debug!(
                        job_id = %job.job_id,
                        vm_id = %event.vm_id,
                        "Ignoring VM failure event for terminal job"
                    );
                    return Ok(());
                }

                let message_text = if event.error_message.is_empty() {
                    "VM startup failed".to_string()
                } else {
                    event.error_message
                };

                tracing::error!(
                    job_id = %job.job_id,
                    vm_id = %event.vm_id,
                    error = %message_text,
                    "Received immediate VM failure event"
                );

                let now = chrono::Utc::now().timestamp();
                self.ctx
                    .job_repo
                    .fail_job(&job.job_id, message_text.clone(), now)
                    .await?;

                let mut updated_job = job;
                updated_job.status = JobStatus::Failed;
                updated_job.stopped_at = Some(now);
                updated_job.error_message = Some(message_text);

                match self.ctx.app_repo.get_app_config(&updated_job.app_id).await {
                    Ok(Some(mut app)) => {
                        let retry_after = now + self.ctx.runtime.restore_retry_backoff_secs;
                        app.restore_retry_after_at = retry_after;
                        update_app_config_best_effort(
                            &self.ctx.app_repo,
                            app,
                            "vm-failure-restore-backoff",
                        )
                        .await;
                    },
                    Ok(None) => {},
                    Err(e) => {
                        tracing::warn!(
                            app_id = %updated_job.app_id,
                            error = %e,
                            "Failed to load app config while handling VM failure"
                        );
                    },
                }

                publish_job_update_best_effort(
                    &self.ctx.nats_client,
                    &updated_job,
                    "vm-failure-job-update",
                )
                .await;
                Ok(())
            })
            .await
    }

    pub async fn cleanup_stale_workers(&self) -> DomainResult<u64> {
        let telemetry = self.ctx.telemetry.clone();
        telemetry
            .observe_result("event", "cleanup_stale_workers", async {
                self.ctx
                    .worker_repo
                    .mark_stale_workers_offline(self.ctx.runtime.worker_stale_threshold_secs)
                    .await
            })
            .await
    }
}
