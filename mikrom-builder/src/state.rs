use crate::builder::BuildResult;
use anyhow::{Context, Result};
use dashmap::DashMap;
use mikrom_proto::builder::{
    BuildEventKind as ProtoBuildEventKind, BuildMetrics as ProtoBuildMetrics,
    BuildRecord as ProtoBuildRecord, BuildStatus, GetBuildMetricsResponse,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

const STATE_VERSION: u32 = 2;
const MAX_EVENTS_PER_BUILD: usize = 32;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BuildEvent {
    pub at_unix: i64,
    pub kind: BuildEventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildEventKind {
    Queued,
    GitMetadataCaptured,
    BuildSucceeded,
    BuildFailed,
    BuildCancelled,
    ExpiredRemoved,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BuildMetrics {
    pub total_started: u64,
    pub total_succeeded: u64,
    pub total_failed: u64,
    pub total_cancelled: u64,
    pub total_expired_removed: u64,
    pub active_builds: u64,
    pub events_recorded: u64,
    pub average_duration_ms: f64,
    pub max_duration_ms: u64,
    pub last_event_at_unix: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BuildRecord {
    pub id: String,
    pub status: i32,
    pub image_tag: Option<String>,
    pub message: Option<String>,
    pub exposed_port: u32,
    pub git_commit_hash: Option<String>,
    pub git_commit_message: Option<String>,
    pub git_branch: Option<String>,
    pub created_at_unix: i64,
    pub completed_at_unix: Option<i64>,
    #[serde(default)]
    pub completed_duration_ms: Option<u64>,
    #[serde(default)]
    pub events: Vec<BuildEvent>,
}

impl BuildRecord {
    pub fn new(id: String) -> Self {
        let mut record = Self {
            id,
            status: BuildStatus::Building as i32,
            image_tag: None,
            message: None,
            exposed_port: 0,
            git_commit_hash: None,
            git_commit_message: None,
            git_branch: None,
            created_at_unix: now_unix(),
            completed_at_unix: None,
            completed_duration_ms: None,
            events: Vec::new(),
        };
        record.push_event(
            BuildEventKind::Queued,
            Some("Build queued".to_string()),
            None,
        );
        record
    }

    pub fn push_event(
        &mut self,
        kind: BuildEventKind,
        message: Option<String>,
        percent: Option<f32>,
    ) {
        self.events.push(BuildEvent {
            at_unix: now_unix(),
            kind,
            message,
            percent,
        });
        if self.events.len() > MAX_EVENTS_PER_BUILD {
            let overflow = self.events.len() - MAX_EVENTS_PER_BUILD;
            self.events.drain(0..overflow);
        }
    }

    pub fn set_git_metadata(
        &mut self,
        git_commit_hash: String,
        git_commit_message: String,
        git_branch: String,
    ) {
        self.git_commit_hash = Some(git_commit_hash);
        self.git_commit_message = Some(git_commit_message);
        self.git_branch = Some(git_branch);
        self.push_event(
            BuildEventKind::GitMetadataCaptured,
            Some("Git metadata captured".to_string()),
            Some(10.0),
        );
    }

    pub fn finalize_success(&mut self, result: &BuildResult) {
        self.status = BuildStatus::Success as i32;
        self.image_tag = Some(result.image_tag.clone());
        self.exposed_port = result.exposed_port;
        self.message = Some("Build successful".to_string());
        self.completed_at_unix = Some(now_unix());
        self.completed_duration_ms = Some(self.duration_ms().unwrap_or_default());
        self.push_event(
            BuildEventKind::BuildSucceeded,
            Some("Build completed successfully".to_string()),
            Some(100.0),
        );
    }

    pub fn finalize_failure(&mut self, message: String, cancelled: bool) {
        self.status = BuildStatus::Failed as i32;
        self.message = Some(message.clone());
        self.completed_at_unix = Some(now_unix());
        self.completed_duration_ms = Some(self.duration_ms().unwrap_or_default());
        self.push_event(
            if cancelled {
                BuildEventKind::BuildCancelled
            } else {
                BuildEventKind::BuildFailed
            },
            Some(message),
            None,
        );
    }

    pub fn as_status(&self) -> BuildStatus {
        if self.status == BuildStatus::Building as i32 {
            BuildStatus::Building
        } else if self.status == BuildStatus::Success as i32 {
            BuildStatus::Success
        } else {
            BuildStatus::Failed
        }
    }

    pub fn completed_at(&self) -> Option<SystemTime> {
        self.completed_at_unix
            .and_then(|ts| u64::try_from(ts).ok())
            .map(|secs| UNIX_EPOCH + Duration::from_secs(secs))
    }

    pub fn duration_ms(&self) -> Option<u64> {
        let completed_at = self.completed_at()?;
        let created_at = u64::try_from(self.created_at_unix).ok()?;
        let created = UNIX_EPOCH + Duration::from_secs(created_at);
        completed_at
            .duration_since(created)
            .ok()
            .and_then(|duration| u64::try_from(duration.as_millis()).ok())
    }

    pub fn is_expired(&self, now: SystemTime, ttl: Duration) -> bool {
        let Some(completed_at) = self.completed_at() else {
            return false;
        };

        now.duration_since(completed_at)
            .map(|elapsed| elapsed >= ttl)
            .unwrap_or(false)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct BuildStateSnapshot {
    version: u32,
    records: Vec<BuildRecord>,
    metrics: BuildMetrics,
}

#[derive(Clone)]
pub struct BuildStore {
    path: PathBuf,
    records: Arc<DashMap<String, BuildRecord>>,
    metrics: Arc<Mutex<BuildMetrics>>,
    io_lock: Arc<Mutex<()>>,
}

impl BuildStore {
    pub async fn load(path: PathBuf) -> Result<Self> {
        let (records, metrics) = if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            let content = tokio::fs::read_to_string(&path)
                .await
                .with_context(|| format!("Failed to read build state {}", path.display()))?;

            if let Ok(snapshot) = serde_json::from_str::<BuildStateSnapshot>(&content) {
                (
                    Self::records_from_snapshot(snapshot.records),
                    snapshot.metrics,
                )
            } else if let Ok(legacy_records) = serde_json::from_str::<Vec<BuildRecord>>(&content) {
                let records = Self::records_from_snapshot(legacy_records);
                let metrics = Self::metrics_from_records(&records);
                (records, metrics)
            } else {
                anyhow::bail!("Failed to parse build state {}", path.display());
            }
        } else {
            (DashMap::new(), BuildMetrics::default())
        };

        let store = Self {
            path,
            records: Arc::new(records),
            metrics: Arc::new(Mutex::new(metrics)),
            io_lock: Arc::new(Mutex::new(())),
        };
        store.persist().await?;
        Ok(store)
    }

    pub fn get(&self, build_id: &str) -> Option<BuildRecord> {
        self.records.get(build_id).map(|entry| entry.clone())
    }

    pub async fn list_records(&self) -> Vec<BuildRecord> {
        self.records
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    pub async fn metrics_snapshot(&self) -> BuildMetrics {
        self.snapshot_metrics().await
    }

    pub async fn insert_new(&self, build_id: String) -> Result<()> {
        self.records
            .insert(build_id.clone(), BuildRecord::new(build_id));
        {
            let mut metrics = self.metrics.lock().await;
            metrics.total_started = metrics.total_started.saturating_add(1);
        }
        self.persist().await
    }

    pub async fn set_git_metadata(
        &self,
        build_id: &str,
        git_commit_hash: String,
        git_commit_message: String,
        git_branch: String,
    ) -> Result<()> {
        if let Some(mut entry) = self.records.get_mut(build_id) {
            entry.set_git_metadata(git_commit_hash, git_commit_message, git_branch);
        }
        self.persist().await
    }

    pub async fn finalize_success(&self, build_id: &str, result: &BuildResult) -> Result<()> {
        if let Some(mut entry) = self.records.get_mut(build_id) {
            entry.finalize_success(result);
        }
        {
            let mut metrics = self.metrics.lock().await;
            metrics.total_succeeded = metrics.total_succeeded.saturating_add(1);
        }
        self.persist().await
    }

    pub async fn finalize_failure(&self, build_id: &str, message: String) -> Result<()> {
        if let Some(mut entry) = self.records.get_mut(build_id) {
            entry.finalize_failure(message, false);
        }
        {
            let mut metrics = self.metrics.lock().await;
            metrics.total_failed = metrics.total_failed.saturating_add(1);
        }
        self.persist().await
    }

    pub async fn finalize_cancelled(&self, build_id: &str, message: String) -> Result<()> {
        if let Some(mut entry) = self.records.get_mut(build_id) {
            entry.finalize_failure(message, true);
        }
        {
            let mut metrics = self.metrics.lock().await;
            metrics.total_cancelled = metrics.total_cancelled.saturating_add(1);
        }
        self.persist().await
    }

    pub async fn remove_expired(&self, ttl: Duration) -> Result<usize> {
        let now = SystemTime::now();
        let expired: Vec<String> = self
            .records
            .iter()
            .filter(|entry| entry.value().is_expired(now, ttl))
            .map(|entry| entry.key().clone())
            .collect();

        let count = expired.len();
        for build_id in expired {
            self.records.remove(&build_id);
        }
        if count > 0 {
            {
                let mut metrics = self.metrics.lock().await;
                metrics.total_expired_removed =
                    metrics.total_expired_removed.saturating_add(count as u64);
            }
            self.persist().await?;
        }
        Ok(count)
    }

    pub async fn persist(&self) -> Result<()> {
        let _guard = self.io_lock.lock().await;
        let snapshot_records: Vec<BuildRecord> = self
            .records
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        let metrics_snapshot = {
            let mut metrics = self.metrics.lock().await;
            Self::refresh_derived_metrics(&mut metrics, &snapshot_records);
            metrics.clone()
        };

        let snapshot = BuildStateSnapshot {
            version: STATE_VERSION,
            records: snapshot_records,
            metrics: metrics_snapshot,
        };

        let payload =
            serde_json::to_vec_pretty(&snapshot).context("Failed to serialize build state")?;

        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("Failed to create state dir {}", parent.display()))?;
        }

        let tmp_path = self.path.with_extension("json.tmp");
        tokio::fs::write(&tmp_path, payload)
            .await
            .with_context(|| format!("Failed to write temporary state {}", tmp_path.display()))?;
        tokio::fs::rename(&tmp_path, &self.path)
            .await
            .with_context(|| format!("Failed to commit build state {}", self.path.display()))?;
        Ok(())
    }

    async fn snapshot_metrics(&self) -> BuildMetrics {
        let snapshot_records: Vec<BuildRecord> = self
            .records
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        let mut metrics = self.metrics.lock().await.clone();
        Self::refresh_derived_metrics(&mut metrics, &snapshot_records);
        metrics
    }

    fn records_from_snapshot(records: Vec<BuildRecord>) -> DashMap<String, BuildRecord> {
        let map = DashMap::new();
        for mut record in records {
            if record.status == BuildStatus::Building as i32 {
                record.finalize_failure(
                    "Builder restarted while build was in progress".to_string(),
                    false,
                );
            }
            map.insert(record.id.clone(), record);
        }
        map
    }

    fn metrics_from_records(records: &DashMap<String, BuildRecord>) -> BuildMetrics {
        let snapshot: Vec<BuildRecord> =
            records.iter().map(|entry| entry.value().clone()).collect();
        let mut metrics = BuildMetrics {
            total_started: snapshot.len() as u64,
            total_succeeded: snapshot
                .iter()
                .filter(|record| record.status == BuildStatus::Success as i32)
                .count() as u64,
            total_failed: snapshot
                .iter()
                .filter(|record| {
                    record.status == BuildStatus::Failed as i32
                        && !record
                            .message
                            .as_deref()
                            .unwrap_or("")
                            .to_ascii_lowercase()
                            .contains("cancel")
                })
                .count() as u64,
            total_cancelled: snapshot
                .iter()
                .filter(|record| {
                    record.status == BuildStatus::Failed as i32
                        && record
                            .message
                            .as_deref()
                            .unwrap_or("")
                            .to_ascii_lowercase()
                            .contains("cancel")
                })
                .count() as u64,
            ..Default::default()
        };
        Self::refresh_derived_metrics(&mut metrics, &snapshot);
        metrics
    }

    fn refresh_derived_metrics(metrics: &mut BuildMetrics, records: &[BuildRecord]) {
        metrics.active_builds = records
            .iter()
            .filter(|record| record.status == BuildStatus::Building as i32)
            .count() as u64;
        metrics.events_recorded = records
            .iter()
            .map(|record| record.events.len() as u64)
            .sum();

        let durations: Vec<u64> = records
            .iter()
            .filter_map(|record| record.duration_ms())
            .collect();
        if durations.is_empty() {
            metrics.average_duration_ms = 0.0;
            metrics.max_duration_ms = 0;
        } else {
            let sum: u64 = durations.iter().sum();
            metrics.average_duration_ms = sum as f64 / durations.len() as f64;
            metrics.max_duration_ms = durations.iter().copied().max().unwrap_or(0);
        }

        metrics.last_event_at_unix = records
            .iter()
            .flat_map(|record| record.events.iter().map(|event| event.at_unix))
            .max();
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

pub fn status_response_from_record(
    record: &BuildRecord,
) -> mikrom_proto::builder::GetBuildStatusResponse {
    mikrom_proto::builder::GetBuildStatusResponse {
        build_id: record.id.clone(),
        status: record.as_status() as i32,
        image_tag: record.image_tag.clone().unwrap_or_default(),
        message: record.message.clone().unwrap_or_default(),
        exposed_port: record.exposed_port,
        git_commit_hash: record.git_commit_hash.clone().unwrap_or_default(),
        git_commit_message: record.git_commit_message.clone().unwrap_or_default(),
        git_branch: record.git_branch.clone().unwrap_or_default(),
    }
}

pub fn success_response_from_result(
    build_id: String,
    record: &BuildRecord,
    result: &BuildResult,
) -> mikrom_proto::builder::GetBuildStatusResponse {
    mikrom_proto::builder::GetBuildStatusResponse {
        build_id,
        status: BuildStatus::Success as i32,
        image_tag: result.image_tag.clone(),
        exposed_port: result.exposed_port,
        git_commit_hash: record.git_commit_hash.clone().unwrap_or_default(),
        git_commit_message: record.git_commit_message.clone().unwrap_or_default(),
        git_branch: record.git_branch.clone().unwrap_or_default(),
        message: "Build successful".to_string(),
    }
}

pub fn failure_response(
    build_id: String,
    message: String,
) -> mikrom_proto::builder::GetBuildStatusResponse {
    mikrom_proto::builder::GetBuildStatusResponse {
        build_id,
        status: BuildStatus::Failed as i32,
        message,
        ..Default::default()
    }
}

pub fn metrics_response_from_store(
    metrics: &BuildMetrics,
    records: &[BuildRecord],
) -> GetBuildMetricsResponse {
    GetBuildMetricsResponse {
        metrics: Some(proto_metrics_from_metrics(metrics)),
        records: records.iter().map(proto_record_from_record).collect(),
    }
}

pub fn proto_metrics_from_metrics(metrics: &BuildMetrics) -> ProtoBuildMetrics {
    ProtoBuildMetrics {
        total_started: metrics.total_started,
        total_succeeded: metrics.total_succeeded,
        total_failed: metrics.total_failed,
        total_cancelled: metrics.total_cancelled,
        total_expired_removed: metrics.total_expired_removed,
        active_builds: metrics.active_builds,
        events_recorded: metrics.events_recorded,
        average_duration_ms: metrics.average_duration_ms,
        max_duration_ms: metrics.max_duration_ms,
        last_event_at_unix: metrics.last_event_at_unix.unwrap_or_default(),
    }
}

pub fn proto_record_from_record(record: &BuildRecord) -> ProtoBuildRecord {
    ProtoBuildRecord {
        id: record.id.clone(),
        status: record.as_status() as i32,
        image_tag: record.image_tag.clone().unwrap_or_default(),
        message: record.message.clone().unwrap_or_default(),
        exposed_port: record.exposed_port,
        git_commit_hash: record.git_commit_hash.clone().unwrap_or_default(),
        git_commit_message: record.git_commit_message.clone().unwrap_or_default(),
        git_branch: record.git_branch.clone().unwrap_or_default(),
        created_at_unix: record.created_at_unix,
        completed_at_unix: record.completed_at_unix,
        completed_duration_ms: record.completed_duration_ms,
        events: record.events.iter().map(proto_event_from_event).collect(),
    }
}

fn proto_event_from_event(event: &BuildEvent) -> mikrom_proto::builder::BuildEvent {
    mikrom_proto::builder::BuildEvent {
        at_unix: event.at_unix,
        kind: match event.kind {
            BuildEventKind::Queued => ProtoBuildEventKind::Queued as i32,
            BuildEventKind::GitMetadataCaptured => ProtoBuildEventKind::GitMetadataCaptured as i32,
            BuildEventKind::BuildSucceeded => ProtoBuildEventKind::BuildSucceeded as i32,
            BuildEventKind::BuildFailed => ProtoBuildEventKind::BuildFailed as i32,
            BuildEventKind::BuildCancelled => ProtoBuildEventKind::BuildCancelled as i32,
            BuildEventKind::ExpiredRemoved => ProtoBuildEventKind::ExpiredRemoved as i32,
        },
        message: event.message.clone().unwrap_or_default(),
        percent: event.percent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_store_persists_and_reloads() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("builds.json");

        let store = BuildStore::load(path.clone()).await.unwrap();
        store.insert_new("build-1".to_string()).await.unwrap();
        store
            .set_git_metadata(
                "build-1",
                "abc123".to_string(),
                "commit msg".to_string(),
                "main".to_string(),
            )
            .await
            .unwrap();

        let result = BuildResult {
            image_tag: "localhost:5000/app:latest".to_string(),
            exposed_port: 8080,
        };
        store.finalize_success("build-1", &result).await.unwrap();

        let reloaded = BuildStore::load(path).await.unwrap();
        let reloaded_record = reloaded.get("build-1").unwrap();
        assert_eq!(
            reloaded_record.image_tag.as_deref(),
            Some("localhost:5000/app:latest")
        );
        assert_eq!(reloaded_record.git_branch.as_deref(), Some("main"));
        assert!(reloaded_record.events.len() >= 3);
        assert_eq!(reloaded.metrics_snapshot().await.total_succeeded, 1);
    }

    #[tokio::test]
    async fn test_store_converts_inflight_builds_on_reload() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("builds.json");
        let raw = vec![BuildRecord::new("build-2".to_string())];
        tokio::fs::write(&path, serde_json::to_vec_pretty(&raw).unwrap())
            .await
            .unwrap();

        let store = BuildStore::load(path).await.unwrap();
        let record = store.get("build-2").unwrap();
        assert_eq!(record.as_status(), BuildStatus::Failed);
        assert!(
            record
                .message
                .as_deref()
                .unwrap_or("")
                .contains("restarted")
        );
    }

    #[tokio::test]
    async fn test_store_tracks_event_log_and_metrics() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("builds.json");
        let store = BuildStore::load(path).await.unwrap();

        store.insert_new("build-3".to_string()).await.unwrap();
        store
            .set_git_metadata(
                "build-3",
                "deadbeef".to_string(),
                "commit".to_string(),
                "feature/refactor".to_string(),
            )
            .await
            .unwrap();
        store
            .finalize_cancelled("build-3", "Build cancelled by shutdown".to_string())
            .await
            .unwrap();

        let record = store.get("build-3").unwrap();
        assert!(
            record
                .events
                .iter()
                .any(|event| matches!(event.kind, BuildEventKind::Queued))
        );
        assert!(
            record
                .events
                .iter()
                .any(|event| matches!(event.kind, BuildEventKind::GitMetadataCaptured))
        );
        assert!(
            record
                .events
                .iter()
                .any(|event| matches!(event.kind, BuildEventKind::BuildCancelled))
        );

        let metrics = store.metrics_snapshot().await;
        assert_eq!(metrics.total_started, 1);
        assert_eq!(metrics.total_cancelled, 1);
        assert_eq!(metrics.active_builds, 0);
        assert!(metrics.events_recorded >= 3);
        assert!(metrics.last_event_at_unix.is_some());
    }

    #[tokio::test]
    async fn test_metrics_response_uses_protobuf_shapes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("builds.json");
        let store = BuildStore::load(path).await.unwrap();

        store.insert_new("build-4".to_string()).await.unwrap();
        store
            .set_git_metadata(
                "build-4",
                "facefeed".to_string(),
                "message".to_string(),
                "main".to_string(),
            )
            .await
            .unwrap();
        let result = BuildResult {
            image_tag: "localhost:5000/app:latest".to_string(),
            exposed_port: 8080,
        };
        store.finalize_success("build-4", &result).await.unwrap();

        let metrics = store.metrics_snapshot().await;
        let records = store.list_records().await;
        let response = metrics_response_from_store(&metrics, &records);

        let response_metrics = response.metrics.expect("metrics payload");
        assert_eq!(response_metrics.total_started, 1);
        assert_eq!(response_metrics.total_succeeded, 1);
        assert_eq!(response.records.len(), 1);
        assert_eq!(response.records[0].events.len(), 3);
        assert_eq!(response.records[0].image_tag, "localhost:5000/app:latest");
    }
}
