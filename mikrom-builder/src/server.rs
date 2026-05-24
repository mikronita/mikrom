use crate::builder::{AppBuilder, GitMetadata};
use crate::state::{
    BuildStore, failure_response, metrics_response_from_store, status_response_from_record,
    success_response_from_result,
};
use dashmap::DashMap;
use futures::StreamExt;
use mikrom_proto::builder::{
    BuildProgress, BuildRequest, BuildResponse, BuildStatus, GetBuildMetricsRequest,
    GetBuildStatusRequest, GetBuildStatusResponse,
};
use prost::Message;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Semaphore, mpsc};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Clone)]
pub struct BuilderServer {
    builder: Arc<AppBuilder>,
    store: Arc<BuildStore>,
    build_limiter: Arc<Semaphore>,
    active_builds: Arc<DashMap<String, CancellationToken>>,
    build_state_ttl: Duration,
}

struct BuildLease {
    build_id: String,
    active_builds: Arc<DashMap<String, CancellationToken>>,
}

impl Drop for BuildLease {
    fn drop(&mut self) {
        self.active_builds.remove(&self.build_id);
    }
}

impl BuilderServer {
    pub async fn new(
        builder: AppBuilder,
        max_concurrent_builds: usize,
        build_state_ttl: Duration,
        build_state_path: PathBuf,
    ) -> anyhow::Result<Self> {
        let store = BuildStore::load(build_state_path).await?;
        Ok(Self {
            builder: Arc::new(builder),
            store: Arc::new(store),
            build_limiter: Arc::new(Semaphore::new(max_concurrent_builds.max(1))),
            active_builds: Arc::new(DashMap::new()),
            build_state_ttl,
        })
    }

    pub async fn listen(
        &self,
        nats_client: async_nats::Client,
        shutdown: CancellationToken,
    ) -> anyhow::Result<()> {
        info!("Starting BuilderServer listeners...");

        let build_server = self.clone();
        let status_server = self.clone();
        let cleanup_server = self.clone();
        let build_nats = nats_client.clone();
        let status_nats = nats_client.clone();
        let build_shutdown = shutdown.clone();
        let status_shutdown = shutdown.clone();
        let cleanup_shutdown = shutdown.clone();

        let build_task = tokio::spawn(async move {
            build_server
                .start_build_worker(build_nats, build_shutdown)
                .await
        });
        let status_task = tokio::spawn(async move {
            status_server
                .start_status_worker(status_nats, status_shutdown)
                .await
        });
        let metrics_server = self.clone();
        let metrics_nats_client = nats_client.clone();
        let metrics_shutdown = shutdown.clone();
        let metrics_task = tokio::spawn(async move {
            metrics_server
                .start_metrics_worker(metrics_nats_client, metrics_shutdown)
                .await
        });
        let cleanup_task =
            tokio::spawn(
                async move { cleanup_server.start_cleanup_worker(cleanup_shutdown).await },
            );

        shutdown.cancelled().await;
        info!("Shutdown requested, cancelling active builds");
        self.cancel_active_builds();

        let join_timeout = tokio::time::timeout(Duration::from_secs(10), async {
            let _ = build_task.await;
            let _ = status_task.await;
            let _ = metrics_task.await;
            let _ = cleanup_task.await;
        })
        .await;

        if join_timeout.is_err() {
            warn!("Timed out waiting for builder tasks to stop cleanly");
        }

        Ok(())
    }

    fn cancel_active_builds(&self) {
        for entry in self.active_builds.iter() {
            entry.value().cancel();
        }
    }

    async fn start_build_worker(
        &self,
        nats: async_nats::Client,
        shutdown: CancellationToken,
    ) -> anyhow::Result<()> {
        let mut subscription = nats
            .queue_subscribe("mikrom.builder.build", "builders".to_string())
            .await
            .map_err(|e| anyhow::anyhow!("Build subscription failed: {}", e))?;

        info!("Listening for build requests on mikrom.builder.build (Queue Group: builders)");

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                message = subscription.next() => {
                    let Some(message) = message else {
                        break;
                    };

                    let permit = tokio::select! {
                        _ = shutdown.cancelled() => break,
                        permit = self.build_limiter.clone().acquire_owned() => {
                            permit.map_err(|e| anyhow::anyhow!("Build limiter closed: {}", e))?
                        }
                    };

                    let nats = nats.clone();
                    let builder = self.builder.clone();
                    let store = self.store.clone();
                    let active_builds = self.active_builds.clone();
                    let build_cancel = shutdown.child_token();

                    tokio::spawn(async move {
                        let _permit = permit;
                        if let Err(e) = Self::handle_build_request(
                            nats,
                            builder,
                            store,
                            active_builds,
                            build_cancel,
                            message,
                        )
                        .await
                        {
                            error!("Error handling build request: {}", e);
                        }
                    });
                }
            }
        }

        Ok(())
    }

    async fn handle_build_request(
        nats: async_nats::Client,
        builder: Arc<AppBuilder>,
        store: Arc<BuildStore>,
        active_builds: Arc<DashMap<String, CancellationToken>>,
        build_cancel: CancellationToken,
        message: async_nats::Message,
    ) -> anyhow::Result<()> {
        let req = BuildRequest::decode(&message.payload[..])
            .map_err(|e| anyhow::anyhow!("Failed to decode BuildRequest: {}", e))?;

        let build_id = Uuid::new_v4().to_string();
        info!(build_id = %build_id, app_id = %req.app_id, "Received build request");

        let lease = BuildLease {
            build_id: build_id.clone(),
            active_builds,
        };
        lease
            .active_builds
            .insert(build_id.clone(), build_cancel.clone());

        if let Some(reply) = message.reply {
            let resp = BuildResponse {
                success: true,
                build_id: build_id.clone(),
                message: "Build started".to_string(),
            };
            let _ = nats.publish(reply, resp.encode_to_vec().into()).await;
        }

        if let Err(e) = store.insert_new(build_id.clone()).await {
            error!(build_id = %build_id, error = %e, "Failed to persist initial build state");
        }

        let (tx, rx) = mpsc::channel::<GitMetadata>(1);
        tokio::spawn(Self::monitor_git_metadata(
            build_id.clone(),
            store.clone(),
            nats.clone(),
            rx,
        ));

        let result = builder
            .build_image(
                &req.app_id,
                &req.git_url,
                req.git_auth_token,
                &req.image_name,
                &req.tag,
                build_cancel.clone(),
                Some(tx),
            )
            .await;

        match result {
            Ok(result) => {
                if let Err(e) = store.finalize_success(&build_id, &result).await {
                    error!(build_id = %build_id, error = %e, "Failed to persist successful build state");
                }
                if let Some(record) = store.get(&build_id) {
                    let status_response =
                        success_response_from_result(build_id.clone(), &record, &result);
                    let _ = nats
                        .publish(
                            format!("mikrom.builder.{}.status", build_id),
                            status_response.encode_to_vec().into(),
                        )
                        .await;
                }
            },
            Err(e) => {
                let err_msg = e.to_string();
                let cancelled = err_msg.to_ascii_lowercase().contains("cancel");
                let persist_result = if cancelled {
                    store.finalize_cancelled(&build_id, err_msg.clone()).await
                } else {
                    store.finalize_failure(&build_id, err_msg.clone()).await
                };
                if let Err(persist_error) = persist_result {
                    error!(build_id = %build_id, error = %persist_error, "Failed to persist failed build state");
                }
                let status_response = failure_response(build_id.clone(), err_msg);
                let _ = nats
                    .publish(
                        format!("mikrom.builder.{}.status", build_id),
                        status_response.encode_to_vec().into(),
                    )
                    .await;
                return Ok(());
            },
        }

        drop(lease);
        Ok(())
    }

    async fn start_metrics_worker(
        &self,
        nats: async_nats::Client,
        shutdown: CancellationToken,
    ) -> anyhow::Result<()> {
        let mut subscription = nats
            .queue_subscribe("mikrom.builder.get_metrics", "builders".to_string())
            .await
            .map_err(|e| anyhow::anyhow!("Metrics subscription failed: {}", e))?;

        info!("Listening for build metrics requests on mikrom.builder.get_metrics");

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                message = subscription.next() => {
                    let Some(message) = message else {
                        break;
                    };
                    let nats = nats.clone();
                    let store = self.store.clone();
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_metrics_request(nats, store, message).await {
                            error!("Error handling metrics request: {}", e);
                        }
                    });
                }
            }
        }

        Ok(())
    }

    async fn handle_metrics_request(
        nats: async_nats::Client,
        store: Arc<BuildStore>,
        message: async_nats::Message,
    ) -> anyhow::Result<()> {
        let reply = message
            .reply
            .ok_or_else(|| anyhow::anyhow!("Metrics request missing reply subject"))?;

        let _req = GetBuildMetricsRequest::decode(&message.payload[..])
            .map_err(|e| anyhow::anyhow!("Failed to decode GetBuildMetricsRequest: {}", e))?;
        let metrics = store.metrics_snapshot().await;
        let records = store.list_records().await;
        let response = metrics_response_from_store(&metrics, &records);
        let payload = response.encode_to_vec();

        nats.publish(reply, payload.into())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to publish metrics response: {}", e))?;

        Ok(())
    }

    async fn monitor_git_metadata(
        build_id: String,
        store: Arc<BuildStore>,
        nats: async_nats::Client,
        mut rx: mpsc::Receiver<GitMetadata>,
    ) {
        if let Some(meta) = rx.recv().await {
            if let Err(e) = store
                .set_git_metadata(
                    &build_id,
                    meta.hash.clone(),
                    meta.message.clone(),
                    meta.branch.clone(),
                )
                .await
            {
                error!(build_id = %build_id, error = %e, "Failed to persist git metadata");
            }

            let progress = BuildProgress {
                build_id: build_id.clone(),
                message: format!("Cloned {} on {}", meta.hash, meta.branch),
                percent: 10.0,
            };

            let _ = nats
                .publish(
                    format!("mikrom.builder.{}.progress", build_id),
                    progress.encode_to_vec().into(),
                )
                .await;

            if let Some(record) = store.get(&build_id) {
                let status = status_response_from_record(&record);
                let _ = nats
                    .publish(
                        format!("mikrom.builder.{}.status", build_id),
                        status.encode_to_vec().into(),
                    )
                    .await;
            }
        }
    }

    async fn start_status_worker(
        &self,
        nats: async_nats::Client,
        shutdown: CancellationToken,
    ) -> anyhow::Result<()> {
        let mut subscription = nats
            .queue_subscribe("mikrom.builder.get_status", "builders".to_string())
            .await
            .map_err(|e| anyhow::anyhow!("Status subscription failed: {}", e))?;

        info!("Listening for build status requests on mikrom.builder.get_status");

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                message = subscription.next() => {
                    let Some(message) = message else {
                        break;
                    };
                    let nats = nats.clone();
                    let store = self.store.clone();
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_status_request(nats, store, message).await {
                            error!("Error handling status request: {}", e);
                        }
                    });
                }
            }
        }

        Ok(())
    }

    async fn handle_status_request(
        nats: async_nats::Client,
        store: Arc<BuildStore>,
        message: async_nats::Message,
    ) -> anyhow::Result<()> {
        let reply = message
            .reply
            .ok_or_else(|| anyhow::anyhow!("Status request missing reply subject"))?;

        let req = GetBuildStatusRequest::decode(&message.payload[..])
            .map_err(|e| anyhow::anyhow!("Failed to decode GetBuildStatusRequest: {}", e))?;

        let resp = match store.get(&req.build_id) {
            Some(record) => status_response_from_record(&record),
            None => GetBuildStatusResponse {
                build_id: req.build_id,
                status: BuildStatus::Failed as i32,
                message: "Build not found".to_string(),
                ..Default::default()
            },
        };

        nats.publish(reply, resp.encode_to_vec().into())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to publish status response: {}", e))?;

        Ok(())
    }

    async fn start_cleanup_worker(&self, shutdown: CancellationToken) -> anyhow::Result<()> {
        let ttl = self.build_state_ttl;
        let store = self.store.clone();
        let period = ttl.min(Duration::from_secs(60)).max(Duration::from_secs(1));
        let mut interval = tokio::time::interval(period);

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = interval.tick() => {
                    let removed = store.remove_expired(ttl).await?;
                    if removed > 0 {
                        info!(removed, "Removed expired build state entries");
                    }
                }
            }
        }

        Ok(())
    }
}
