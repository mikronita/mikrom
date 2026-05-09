use async_nats::Client;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::time::{Duration, interval};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub vm_id: String,
    pub app_id: String,
    pub source: String, // "stdout" or "stderr"
    pub message: String,
    pub timestamp: i64,
}

pub struct LogShipper {
    vm_id: String,
    app_id: String,
    nats_client: Option<Client>,
    batch_size: usize,
    flush_interval: Duration,
    /// Local cache for the API to query via SSE if needed, or for debugging
    logs_map: std::sync::Arc<dashmap::DashMap<String, VecDeque<String>>>,
}

impl LogShipper {
    pub fn new(
        vm_id: String,
        app_id: String,
        nats_client: Option<Client>,
        logs_map: std::sync::Arc<dashmap::DashMap<String, VecDeque<String>>>,
    ) -> Self {
        Self {
            vm_id,
            app_id,
            nats_client,
            batch_size: 50,
            flush_interval: Duration::from_millis(500),
            logs_map,
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
                .entry(self.vm_id.clone())
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
                vm_id: self.vm_id.clone(),
                app_id: self.app_id.clone(),
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
                        tracing::error!("Failed to publish logs to NATS: {e}");
                    }
                },
                Err(e) => {
                    tracing::error!("Failed to serialize log batch: {e}");
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
        let logs_map = Arc::new(dashmap::DashMap::new());
        let vm_id = "test-vm".to_string();
        let app_id = "test-app".to_string();

        let shipper = LogShipper::new(vm_id.clone(), app_id.clone(), None, logs_map.clone());
        let mut batch = Vec::new();

        shipper
            .process_line("stdout", "Hello World".to_string(), &mut batch, true)
            .await;
        shipper
            .process_line("stderr", "Error Occurred".to_string(), &mut batch, true)
            .await;

        let buffer = logs_map.get(&vm_id).unwrap();

        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer[0], "Hello World");
        assert_eq!(buffer[1], "[stderr] Error Occurred");

        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].message, "Hello World");
        assert_eq!(batch[1].source, "stderr");
    }

    #[tokio::test]
    async fn test_log_shipper_system_filtering() {
        let logs_map = Arc::new(dashmap::DashMap::new());
        let vm_id = "test-vm".to_string();
        let app_id = "test-app".to_string();

        let shipper = LogShipper::new(vm_id.clone(), app_id.clone(), None, logs_map.clone());
        let mut batch = Vec::new();

        // System logs (before marker)
        shipper
            .process_line("stdout", "Booting kernel...".to_string(), &mut batch, false)
            .await;

        // App logs (after marker)
        shipper
            .process_line("stdout", "App started".to_string(), &mut batch, true)
            .await;

        let buffer = logs_map.get(&vm_id).unwrap();

        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer[0], "[system] Booting kernel...");
        assert_eq!(buffer[1], "App started");

        // ONLY "App started" should be in the batch (to be sent to NATS)
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].message, "App started");
    }

    #[tokio::test]
    async fn test_log_shipper_spawn() {
        let logs_map = Arc::new(dashmap::DashMap::new());
        let vm_id = "test-vm".to_string();
        let app_id = "test-app".to_string();

        let shipper = LogShipper::new(vm_id.clone(), app_id.clone(), None, logs_map.clone());

        let (mut stdout_tx, stdout_rx) = tokio::io::duplex(1024);
        let (mut stderr_tx, stderr_rx) = tokio::io::duplex(1024);

        let handle = shipper.spawn(stdout_rx, stderr_rx).await;

        stdout_tx.write_all(b"line 1\n").await.unwrap();
        stderr_tx.write_all(b"error 1\n").await.unwrap();

        // Wait a bit for processing
        tokio::time::sleep(Duration::from_millis(100)).await;

        {
            let buffer = logs_map.get(&vm_id).unwrap();
            assert!(buffer.contains(&"[system] line 1".to_string()));
            assert!(buffer.contains(&"[system-err] error 1".to_string()));
        }

        // Close streams
        drop(stdout_tx);
        drop(stderr_tx);

        handle.await.unwrap();
    }
}
