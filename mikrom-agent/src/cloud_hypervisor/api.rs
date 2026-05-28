use crate::hypervisor::HypervisorError;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Send a request to the Cloud Hypervisor API socket.
/// This implementation is robust against slow responses and handles the full HTTP lifecycle.
#[tracing::instrument(skip_all, fields(method = %method, api_path = %api_path))]
pub async fn ch_request(
    method: &str,
    socket_path: &str,
    api_path: &str,
    body: Option<&str>,
) -> Result<String, HypervisorError> {
    let stream_fut = UnixStream::connect(socket_path);
    let mut stream = tokio::time::timeout(Duration::from_secs(5), stream_fut)
        .await
        .map_err(|_| HypervisorError::ApiError {
            path: api_path.to_string(),
            msg: "connect timeout".to_string(),
        })?
        .map_err(|e| HypervisorError::ApiError {
            path: api_path.to_string(),
            msg: format!("connect: {e}"),
        })?;

    let body_str = body.unwrap_or("");
    let request = format!(
        "{method} {api_path} HTTP/1.1\r\n\
         Host: localhost\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n\
         {body_str}",
        body_str.len()
    );

    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|e| HypervisorError::ApiError {
            path: api_path.to_string(),
            msg: format!("write: {e}"),
        })?;

    // Use a buffered reader to read headers line by line
    let mut reader = BufReader::new(stream);
    let mut status_line = String::new();
    let mut content_length = None;

    // 1. Read status line
    let read_fut = reader.read_line(&mut status_line);
    tokio::time::timeout(Duration::from_secs(30), read_fut)
        .await
        .map_err(|_| HypervisorError::ApiError {
            path: api_path.to_string(),
            msg: "read status line timeout".to_string(),
        })?
        .map_err(|e| HypervisorError::ApiError {
            path: api_path.to_string(),
            msg: format!("read status line: {e}"),
        })?;

    if status_line.is_empty() {
        return Err(HypervisorError::ApiError {
            path: api_path.to_string(),
            msg: "empty response from server".to_string(),
        });
    }

    // 2. Handle 204 No Content immediately
    if status_line.contains(" 204 ") {
        return Ok(String::new());
    }

    // 3. Read headers
    loop {
        let mut line = String::new();
        let read_fut = reader.read_line(&mut line);
        tokio::time::timeout(Duration::from_secs(10), read_fut)
            .await
            .map_err(|_| HypervisorError::ApiError {
                path: api_path.to_string(),
                msg: "read header timeout".to_string(),
            })?
            .map_err(|e| HypervisorError::ApiError {
                path: api_path.to_string(),
                msg: format!("read header: {e}"),
            })?;

        if line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }

        if let Some((key, value)) = line.split_once(':')
            && key.trim().to_lowercase() == "content-length"
        {
            content_length = value.trim().parse::<usize>().ok();
        }
    }

    // 3. Read body
    let mut body_bytes = Vec::new();
    if let Some(len) = content_length {
        body_bytes.resize(len, 0);
        let read_fut = reader.read_exact(&mut body_bytes);
        tokio::time::timeout(Duration::from_secs(60), read_fut)
            .await
            .map_err(|_| HypervisorError::ApiError {
                path: api_path.to_string(),
                msg: "read body timeout".to_string(),
            })?
            .map_err(|e| HypervisorError::ApiError {
                path: api_path.to_string(),
                msg: format!("read body: {e}"),
            })?;
    } else {
        // Fallback: read until EOF if no Content-Length
        let read_fut = reader.read_to_end(&mut body_bytes);
        tokio::time::timeout(Duration::from_secs(60), read_fut)
            .await
            .map_err(|_| HypervisorError::ApiError {
                path: api_path.to_string(),
                msg: "read body to end timeout".to_string(),
            })?
            .map_err(|e| HypervisorError::ApiError {
                path: api_path.to_string(),
                msg: format!("read body to end: {e}"),
            })?;
    }

    let body_text = String::from_utf8_lossy(&body_bytes).to_string();
    let is_success = status_line.contains(" 200 ")
        || status_line.contains(" 201 ")
        || status_line.contains(" 204 ");

    if is_success {
        Ok(body_text)
    } else {
        let err_msg = if body_text.is_empty() {
            status_line.trim().to_string()
        } else {
            format!("{}; body: {}", status_line.trim(), body_text)
        };
        Err(HypervisorError::ApiError {
            path: api_path.to_string(),
            msg: err_msg,
        })
    }
}

pub async fn wait_for_socket(path: &str, timeout: Duration) -> Result<(), HypervisorError> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if tokio::fs::metadata(path).await.is_ok() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(HypervisorError::SocketTimeout(path.to_string()));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::io::AsyncWriteExt;
    use tokio::net::UnixListener;

    async fn spawn_mock_server(
        path: PathBuf,
        response: &'static [u8],
    ) -> tokio::task::JoinHandle<()> {
        let listener = UnixListener::bind(path).unwrap();
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                let _ = stream.write_all(response).await;
                let _ = stream.shutdown().await;
            }
        })
    }

    use std::path::PathBuf;

    #[tokio::test]
    async fn test_ch_request_success() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");
        let socket_str = socket_path.to_string_lossy().to_string();

        let response = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 15\r\n\r\n{\"status\":\"ok\"}";
        let _server = spawn_mock_server(socket_path, response).await;

        let result = ch_request("GET", &socket_str, "/api/v1/info", None)
            .await
            .unwrap();
        assert_eq!(result, "{\"status\":\"ok\"}");
    }

    #[tokio::test]
    async fn test_ch_request_204_no_content() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test-204.sock");
        let socket_str = socket_path.to_string_lossy().to_string();

        let response = b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n";
        let _server = spawn_mock_server(socket_path, response).await;

        let result = ch_request("PUT", &socket_str, "/api/v1/vm.boot", None)
            .await
            .unwrap();
        assert_eq!(result, "");
    }

    #[tokio::test]
    async fn test_ch_request_content_length_parsing() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test-cl.sock");
        let socket_str = socket_path.to_string_lossy().to_string();

        // Response with headers in mixed case and extra whitespace
        let response =
            b"HTTP/1.1 200 OK\r\ncoNTent-lEngth: 5\r\nConnection: close\r\n\r\nhello world";
        let _server = spawn_mock_server(socket_path, response).await;

        let result = ch_request("GET", &socket_str, "/test", None).await.unwrap();
        // Should only read 5 bytes as per Content-Length
        assert_eq!(result, "hello");
    }

    #[tokio::test]
    async fn test_ch_request_error_response() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test-err.sock");
        let socket_str = socket_path.to_string_lossy().to_string();

        let response = b"HTTP/1.1 400 Bad Request\r\nContent-Length: 15\r\n\r\n{\"error\":\"bad\"}";
        let _server = spawn_mock_server(socket_path, response).await;

        let result = ch_request("GET", &socket_str, "/api", None).await;
        assert!(result.is_err());
        if let Err(HypervisorError::ApiError { msg, .. }) = result {
            assert!(msg.contains("400 Bad Request"));
            assert!(msg.contains("{\"error\":\"bad\"}"));
        } else {
            panic!("Expected ApiError");
        }
    }
}
