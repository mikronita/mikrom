use serde_json::{Map, Value};
use std::path::Path;
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::time::timeout;

#[derive(Error, Debug)]
pub enum QmpError {
    #[error("failed to connect to QMP socket: {0}")]
    Connect(String),
    #[error("QMP handshake failed: {0}")]
    Handshake(String),
    #[error("QMP response timed out: {0}")]
    Timeout(String),
    #[error("QMP error response: {0}")]
    Response(String),
}

impl From<std::io::Error> for QmpError {
    fn from(e: std::io::Error) -> Self {
        QmpError::Connect(e.to_string())
    }
}

impl From<serde_json::Error> for QmpError {
    fn from(e: serde_json::Error) -> Self {
        QmpError::Response(e.to_string())
    }
}

type Result<T> = std::result::Result<T, QmpError>;

pub struct QmpClient {
    reader: BufReader<tokio::io::ReadHalf<UnixStream>>,
    writer: tokio::io::WriteHalf<UnixStream>,
}

impl std::fmt::Debug for QmpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QmpClient").finish_non_exhaustive()
    }
}

impl QmpClient {
    pub async fn connect(path: &Path) -> Result<Self> {
        let stream = UnixStream::connect(path).await?;
        let (reader, writer) = tokio::io::split(stream);
        let mut client = Self {
            reader: BufReader::new(reader),
            writer,
        };
        client.handshake().await?;
        Ok(client)
    }

    pub async fn connect_with_timeout(path: &Path, secs: u64) -> Result<Self> {
        timeout(Duration::from_secs(secs), Self::connect(path))
            .await
            .map_err(|_| {
                QmpError::Timeout(format!("QMP connect did not complete within {secs}s"))
            })?
    }

    async fn read_line(&mut self) -> Result<String> {
        let mut line = String::new();
        let _ = self.reader.read_line(&mut line).await?;
        Ok(line.trim().to_string())
    }

    async fn handshake(&mut self) -> Result<()> {
        loop {
            let line = self.read_line().await?;
            if line.is_empty() {
                continue;
            }
            let val: Value = serde_json::from_str(&line)?;
            if val.get("QMP").is_some() {
                break;
            }
        }

        self.send_raw(r#"{"execute":"qmp_capabilities"}"#).await?;
        let resp = self.wait_response().await?;
        if resp.get("error").is_some() {
            return Err(QmpError::Handshake(format!(
                "qmp_capabilities failed: {resp}"
            )));
        }
        Ok(())
    }

    async fn send_raw(&mut self, cmd: &str) -> Result<()> {
        self.writer.write_all(cmd.as_bytes()).await?;
        self.writer.write_all(b"\n").await?;
        self.writer.flush().await?;
        Ok(())
    }

    async fn wait_response(&mut self) -> Result<Value> {
        let deadline = Duration::from_secs(5);
        timeout(deadline, async {
            loop {
                let line = self.read_line().await?;
                if line.is_empty() {
                    continue;
                }
                if let Ok(val) = serde_json::from_str::<Value>(&line) {
                    if val.get("return").is_some() || val.get("error").is_some() {
                        return Ok(val);
                    }
                }
            }
        })
        .await
        .map_err(|_| QmpError::Timeout(format!("no response within {deadline:?}")))?
    }

    pub async fn execute(&mut self, cmd: &str) -> Result<Value> {
        let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
        let mut obj = Map::new();
        obj.insert("execute".into(), Value::String(parts[0].to_string()));
        if parts.len() > 1 {
            let parsed: Value = serde_json::from_str(parts[1])?;
            obj.insert("arguments".into(), parsed);
        }
        let req = Value::Object(obj).to_string();
        self.send_raw(&req).await?;
        let resp = self.wait_response().await?;
        if let Some(err) = resp.get("error") {
            return Err(QmpError::Response(format!("QMP error: {err}")));
        }
        Ok(resp)
    }

    /// Execute a QMP command with structured arguments.
    ///
    /// `cmd` is the command name (e.g. `"blockdev-add"`).
    /// `args` is a JSON object that becomes the `"arguments"` field.
    ///
    /// This is safer than `execute` because it avoids string-interpolation
    /// vulnerabilities when user input is passed in the arguments.
    pub async fn execute_with_args(&mut self, cmd: &str, args: Value) -> Result<Value> {
        let mut obj = Map::new();
        obj.insert("execute".into(), Value::String(cmd.to_string()));
        obj.insert("arguments".into(), args);
        let req = Value::Object(obj).to_string();
        self.send_raw(&req).await?;
        let resp = self.wait_response().await?;
        if let Some(err) = resp.get("error") {
            return Err(QmpError::Response(format!("QMP error: {err}")));
        }
        Ok(resp)
    }

    pub async fn query_status(&mut self) -> Result<Value> {
        self.execute("query-status").await
    }

    pub async fn stop(&mut self) -> Result<Value> {
        self.execute("stop").await
    }

    pub async fn cont(&mut self) -> Result<Value> {
        self.execute("cont").await
    }

    pub async fn system_powerdown(&mut self) -> Result<Value> {
        self.execute("system_powerdown").await
    }

    pub async fn quit(&mut self) -> Result<Value> {
        self.execute("quit").await
    }

    /// Execute a Human Monitor Protocol (HMP) command via QMP.
    ///
    /// QEMU exposes HMP commands through the `human-monitor-command` QMP command.
    /// Returns the raw string output from the monitor (may be empty).
    pub async fn human_monitor_command(&mut self, cmd: &str) -> Result<String> {
        let args = format!(
            "{{\"command-line\":{}}}",
            serde_json::Value::String(cmd.to_string())
        );
        let resp = self
            .execute(&format!("human-monitor-command {args}"))
            .await?;
        match resp.get("return") {
            Some(Value::String(s)) => Ok(s.clone()),
            Some(Value::Null) => Ok(String::new()),
            _ => Ok(String::new()),
        }
    }
}
