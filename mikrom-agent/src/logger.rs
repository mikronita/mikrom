use async_nats::Client;
use chrono::Utc;
use mikrom_proto::id::{AppId, VmId};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::time::{Duration, interval};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub vm_id: VmId,
    pub app_id: AppId,
    pub source: String, // "stdout" or "stderr"
    pub message: String,
    pub timestamp: i64,
}

pub struct LogShipper {
    vm_id: VmId,
    app_id: AppId,
    nats_client: Option<Client>,
    batch_size: usize,
    flush_interval: Duration,
    /// Local cache for the API to query via SSE if needed, or for debugging
    logs_map: std::sync::Arc<dashmap::DashMap<VmId, VecDeque<String>>>,
    app_started: Arc<AtomicBool>,
    app_started_at_ms: Arc<AtomicU64>,
}

impl LogShipper {
    pub fn new(
        vm_id: VmId,
        app_id: AppId,
        nats_client: Option<Client>,
        logs_map: std::sync::Arc<dashmap::DashMap<VmId, VecDeque<String>>>,
        app_started: Arc<AtomicBool>,
        app_started_at_ms: Arc<AtomicU64>,
    ) -> Self {
        Self {
            vm_id,
            app_id,
            nats_client,
            batch_size: 50,
            flush_interval: Duration::from_millis(500),
            logs_map,
            app_started,
            app_started_at_ms,
        }
    }

    pub async fn spawn<R1, R2>(self, stdout: R1, stderr: R2) -> tokio::task::JoinHandle<()>
    where
        R1: tokio::io::AsyncRead + Unpin + Send + 'static,
        R2: tokio::io::AsyncRead + Unpin + Send + 'static,
    {
        tokio::spawn(async move {
            let mut stdout_reader = BufReader::new(stdout).lines();
            let mut stderr_reader = BufReader::new(stderr).lines();
            let mut batch = Vec::new();
            let mut timer = interval(self.flush_interval);
            let mut app_started = false;

            loop {
                tokio::select! {
                    result = stdout_reader.next_line() => {
                        match result {
                            Ok(Some(line)) => {
                                if !app_started && line == "__MIKROM_APP_START__" {
                                    app_started = true;
                                    self.app_started.store(true, Ordering::SeqCst);
                                    self.app_started_at_ms.store(
                                        chrono::Utc::now().timestamp_millis() as u64,
                                        Ordering::SeqCst,
                                    );
                                    tracing::info!(app_id = %self.app_id, vm_id = %self.vm_id, "Application started marker received");
                                    continue;
                                }
                                self.process_line("stdout", line, &mut batch, app_started).await;
                                if batch.len() >= self.batch_size {
                                    self.flush(&mut batch).await;
                                }
                            }
                            _ => break, // Stream closed or error
                        }
                    }
                    result = stderr_reader.next_line() => {
                        match result {
                            Ok(Some(line)) => {
                                self.process_line("stderr", line, &mut batch, app_started).await;
                                if batch.len() >= self.batch_size {
                                    self.flush(&mut batch).await;
                                }
                            }
                            _ => break, // Stream closed or error
                        }
                    }
                    _ = timer.tick() => {
                        if !batch.is_empty() {
                            self.flush(&mut batch).await;
                        }
                    }
                }
            }

            // Final flush before exiting
            if !batch.is_empty() {
                self.flush(&mut batch).await;
            }
        })
    }

    async fn process_line(
        &self,
        source: &str,
        message: String,
        batch: &mut Vec<LogEntry>,
        is_app_log: bool,
    ) {
        // 1. Update local buffer (shared with FirecrackerManager)
        // We ALWAYS update the local buffer for troubleshooting
        {
            let mut buffer = self
                .logs_map
                .entry(self.vm_id)
                .or_insert_with(|| VecDeque::with_capacity(1000));

            if buffer.len() >= 1000 {
                buffer.pop_front();
            }

            let formatted = match (source, is_app_log) {
                ("stderr", true) => format!("[stderr] {message}"),
                ("stderr", false) => {
                    tracing::debug!(vm_id = %self.vm_id, "[system-err] {message}");
                    format!("[system-err] {message}")
                },
                ("stdout", false) => {
                    tracing::debug!(vm_id = %self.vm_id, "[system] {message}");
                    format!("[system] {message}")
                },
                _ => message.clone(),
            };
            buffer.push_back(formatted);
        }

        // 2. Add to NATS batch ONLY if it's an application log
        if is_app_log {
            batch.push(LogEntry {
                vm_id: self.vm_id,
                app_id: self.app_id,
                source: source.to_string(),
                message,
                timestamp: Utc::now().timestamp_nanos_opt().unwrap_or(0),
            });
        }
    }

    async fn flush(&self, batch: &mut Vec<LogEntry>) {
        if let Some(nats) = &self.nats_client {
            let topic = format!("mikrom.logs.{}.{}", self.app_id, self.vm_id);
            match serde_json::to_vec(&batch) {
                Ok(payload) => {
                    if let Err(e) = nats.publish(topic, payload.into()).await {
                        tracing::error!("Failed to publish logs to NATS: {}", e);
                    }
                },
                Err(e) => {
                    tracing::error!("Failed to serialize log batch: {}", e);
                },
            }
        }
        batch.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn test_log_shipper_local_buffer() {
        let vm_id = VmId::new();
        let app_id = AppId::new();
        let logs_map = Arc::new(dashmap::DashMap::new());
        let shipper = LogShipper::new(
            vm_id,
            app_id,
            None,
            logs_map.clone(),
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
        );

        let (mut stdout_writer, stdout_reader) = tokio::io::duplex(64);
        let (mut stderr_writer, stderr_reader) = tokio::io::duplex(64);

        let _handle = shipper.spawn(stdout_reader, stderr_reader).await;

        stdout_writer
            .write_all(b"__MIKROM_APP_START__\n")
            .await
            .unwrap();

        // Give it a moment to process the marker
        tokio::time::sleep(Duration::from_millis(50)).await;

        stdout_writer.write_all(b"Hello Stdout\n").await.unwrap();
        stderr_writer.write_all(b"Hello Stderr\n").await.unwrap();

        // Retry with backoff to handle async processing delays
        let mut buffer_content = Vec::new();
        for i in 0..10 {
            tokio::time::sleep(Duration::from_millis(100 * (i + 1))).await;
            if let Some(buffer) = logs_map.get(&vm_id) {
                buffer_content = buffer.iter().cloned().collect();
                if buffer_content.iter().any(|l| l.contains("Hello Stdout"))
                    && buffer_content
                        .iter()
                        .any(|l| l.contains("[stderr] Hello Stderr"))
                {
                    break;
                }
            }
        }

        assert!(
            buffer_content.iter().any(|l| l.contains("Hello Stdout")),
            "Stdout log not found in buffer: {:?}",
            buffer_content
        );
        assert!(
            buffer_content
                .iter()
                .any(|l| l.contains("[stderr] Hello Stderr")),
            "Stderr log not found in buffer: {:?}",
            buffer_content
        );
    }

    #[tokio::test]
    async fn test_log_shipper_system_logs_marker() {
        let vm_id = VmId::new();
        let app_id = AppId::new();
        let logs_map = Arc::new(dashmap::DashMap::new());
        let shipper = LogShipper::new(
            vm_id,
            app_id,
            None,
            logs_map.clone(),
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
        );

        let (mut stdout_writer, stdout_reader) = tokio::io::duplex(64);
        let (_stderr_writer, stderr_reader) = tokio::io::duplex(64);

        let _handle = shipper.spawn(stdout_reader, stderr_reader).await;

        stdout_writer
            .write_all(b"System Log Before\n")
            .await
            .unwrap();
        stdout_writer
            .write_all(b"__MIKROM_APP_START__\n")
            .await
            .unwrap();
        stdout_writer.write_all(b"App Log After\n").await.unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;

        let buffer = logs_map.get(&vm_id).unwrap();
        assert!(
            buffer
                .iter()
                .any(|l| l.contains("[system] System Log Before"))
        );
        assert!(buffer.iter().any(|l| l == "App Log After"));
    }

    #[tokio::test]
    async fn test_log_shipper_rotates_buffer() {
        let vm_id = VmId::new();
        let app_id = AppId::new();
        let logs_map = Arc::new(dashmap::DashMap::new());
        let shipper = LogShipper::new(
            vm_id,
            app_id,
            None,
            logs_map.clone(),
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
        );

        let (mut stdout_writer, stdout_reader) = tokio::io::duplex(1024);
        let (_stderr_writer, stderr_reader) = tokio::io::duplex(64);

        let _handle = shipper.spawn(stdout_reader, stderr_reader).await;

        for i in 0..1100 {
            stdout_writer
                .write_all(format!("line {}\n", i).as_bytes())
                .await
                .unwrap();
        }

        tokio::time::sleep(Duration::from_millis(200)).await;

        let buffer = logs_map.get(&vm_id).unwrap();
        assert_eq!(buffer.len(), 1000);
        // Should have the LATEST 1000 lines
        assert!(buffer.iter().any(|l| l.contains("line 1099")));
        assert!(!buffer.iter().any(|l| l == "line 0"));
    }
}
