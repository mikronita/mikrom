use crate::firecracker::config::FirecrackerError;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Send a request to the Firecracker API socket and return Ok on 2xx.
#[tracing::instrument(skip_all, fields(method = %method, api_path = %api_path))]
pub async fn fc_request(
    method: &str,
    socket_path: &str,
    api_path: &str,
    body: &str,
) -> Result<(), FirecrackerError> {
    let stream_fut = tokio::net::UnixStream::connect(socket_path);
    let stream = tokio::time::timeout(Duration::from_secs(2), stream_fut)
        .await
        .map_err(|_| FirecrackerError::ApiError {
            path: api_path.to_string(),
            msg: "connect timeout".to_string(),
        })?
        .map_err(|e| FirecrackerError::ApiError {
            path: api_path.to_string(),
            msg: format!("connect: {e}"),
        })?;

    let (reader, mut writer) = tokio::io::split(stream);

    let request = format!(
        "{method} {api_path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );

    writer
        .write_all(request.as_bytes())
        .await
        .map_err(|e| FirecrackerError::ApiError {
            path: api_path.to_string(),
            msg: format!("write: {e}"),
        })?;

    let mut reader = BufReader::new(reader);
    let mut status_line = String::new();

    tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut status_line))
        .await
        .map_err(|_| FirecrackerError::ApiError {
            path: api_path.to_string(),
            msg: "read timeout".to_string(),
        })?
        .map_err(|e| FirecrackerError::ApiError {
            path: api_path.to_string(),
            msg: format!("read: {e}"),
        })?;

    if status_line.contains(" 2") {
        Ok(())
    } else {
        Err(FirecrackerError::ApiError {
            path: api_path.to_string(),
            msg: status_line.trim().to_string(),
        })
    }
}

/// Send a PUT request to the Firecracker API socket and return Ok on 2xx.
#[tracing::instrument(skip_all, fields(api_path = %api_path))]
pub async fn fc_put(socket_path: &str, api_path: &str, body: &str) -> Result<(), FirecrackerError> {
    fc_request("PUT", socket_path, api_path, body).await
}

/// Send a PATCH request to the Firecracker API socket and return Ok on 2xx.
#[tracing::instrument(skip_all, fields(api_path = %api_path))]
pub async fn fc_patch(
    socket_path: &str,
    api_path: &str,
    body: &str,
) -> Result<(), FirecrackerError> {
    fc_request("PATCH", socket_path, api_path, body).await
}

/// Poll until the Unix socket file appears (Firecracker is ready to accept API calls).
pub async fn wait_for_socket(path: &str, timeout: Duration) -> Result<(), FirecrackerError> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if tokio::fs::metadata(path).await.is_ok() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(FirecrackerError::SocketTimeout(path.to_string()));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
