use crate::hypervisor::HypervisorError;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

const MAX_RESPONSE_BODY_BYTES: usize = 10 * 1024 * 1024;

/// Send a request to the Firecracker API socket and return Ok on 2xx.
#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip_all, fields(method = %method, api_path = %api_path))]
pub async fn fc_request(
    method: &str,
    socket_path: &str,
    api_path: &str,
    body: &str,
    connect_timeout: Duration,
    status_timeout: Duration,
    header_timeout: Duration,
    body_timeout: Duration,
) -> Result<(), HypervisorError> {
    let stream_fut = tokio::net::UnixStream::connect(socket_path);
    let stream = tokio::time::timeout(connect_timeout, stream_fut)
        .await
        .map_err(|_| HypervisorError::ApiError {
            path: api_path.to_string(),
            msg: "connect timeout".to_string(),
        })?
        .map_err(|e| HypervisorError::ApiError {
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
        .map_err(|e| HypervisorError::ApiError {
            path: api_path.to_string(),
            msg: format!("write: {e}"),
        })?;

    let mut reader = BufReader::new(reader);
    read_firecracker_response(
        &mut reader,
        api_path,
        status_timeout,
        header_timeout,
        body_timeout,
    )
    .await
}

async fn read_firecracker_response<R>(
    reader: &mut BufReader<R>,
    api_path: &str,
    status_timeout: Duration,
    header_timeout: Duration,
    body_timeout: Duration,
) -> Result<(), HypervisorError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut status_line = String::new();

    tokio::time::timeout(status_timeout, reader.read_line(&mut status_line))
        .await
        .map_err(|_| HypervisorError::ApiError {
            path: api_path.to_string(),
            msg: "read timeout".to_string(),
        })?
        .map_err(|e| HypervisorError::ApiError {
            path: api_path.to_string(),
            msg: format!("read: {e}"),
        })?;

    let mut content_length: Option<usize> = None;

    loop {
        let mut header_line = String::new();
        tokio::time::timeout(header_timeout, reader.read_line(&mut header_line))
            .await
            .map_err(|_| HypervisorError::ApiError {
                path: api_path.to_string(),
                msg: "header read timeout".to_string(),
            })?
            .map_err(|e| HypervisorError::ApiError {
                path: api_path.to_string(),
                msg: format!("header read: {e}"),
            })?;

        if header_line.is_empty() || header_line == "\r\n" {
            break;
        }

        if let Some((name, value)) = header_line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            content_length = value.trim().parse::<usize>().ok();
        }
    }

    let is_success = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .is_some_and(|code| (200..300).contains(&code));

    if is_success && content_length.is_none_or(|len| len == 0) {
        return Ok(());
    }

    let mut response_body = Vec::new();
    match content_length {
        Some(len) => {
            if len > MAX_RESPONSE_BODY_BYTES {
                return Err(HypervisorError::ApiError {
                    path: api_path.to_string(),
                    msg: format!("response body too large: {len} bytes"),
                });
            }
            response_body.resize(len, 0);
            tokio::time::timeout(body_timeout, reader.read_exact(&mut response_body))
                .await
                .map_err(|_| HypervisorError::ApiError {
                    path: api_path.to_string(),
                    msg: "body read timeout".to_string(),
                })?
                .map_err(|e| HypervisorError::ApiError {
                    path: api_path.to_string(),
                    msg: format!("body read: {e}"),
                })?;
        },
        None => {
            let mut limited_reader = reader.take(MAX_RESPONSE_BODY_BYTES as u64 + 1);
            tokio::time::timeout(body_timeout, limited_reader.read_to_end(&mut response_body))
                .await
                .map_err(|_| HypervisorError::ApiError {
                    path: api_path.to_string(),
                    msg: "body read timeout".to_string(),
                })?
                .map_err(|e| HypervisorError::ApiError {
                    path: api_path.to_string(),
                    msg: format!("body read: {e}"),
                })?;

            if response_body.len() > MAX_RESPONSE_BODY_BYTES {
                return Err(HypervisorError::ApiError {
                    path: api_path.to_string(),
                    msg: format!("response body too large: {} bytes", response_body.len()),
                });
            }
        },
    }

    if is_success {
        Ok(())
    } else {
        let body_text = String::from_utf8_lossy(&response_body).trim().to_string();
        let msg = if body_text.is_empty() {
            status_line.trim().to_string()
        } else {
            format!("{}; body: {}", status_line.trim(), body_text)
        };
        Err(HypervisorError::ApiError {
            path: api_path.to_string(),
            msg,
        })
    }
}

/// Send a PUT request to the Firecracker API socket and return Ok on 2xx.
#[tracing::instrument(skip_all, fields(api_path = %api_path))]
pub async fn fc_put(socket_path: &str, api_path: &str, body: &str) -> Result<(), HypervisorError> {
    fc_put_with_timeouts(
        socket_path,
        api_path,
        body,
        Duration::from_secs(2),
        Duration::from_secs(2),
        Duration::from_secs(2),
        Duration::from_secs(2),
    )
    .await
}

/// Send a PATCH request to the Firecracker API socket and return Ok on 2xx.
#[tracing::instrument(skip_all, fields(api_path = %api_path))]
pub async fn fc_patch(
    socket_path: &str,
    api_path: &str,
    body: &str,
) -> Result<(), HypervisorError> {
    fc_patch_with_timeouts(
        socket_path,
        api_path,
        body,
        Duration::from_secs(2),
        Duration::from_secs(2),
        Duration::from_secs(2),
        Duration::from_secs(2),
    )
    .await
}

pub async fn fc_put_with_timeouts(
    socket_path: &str,
    api_path: &str,
    body: &str,
    connect_timeout: Duration,
    status_timeout: Duration,
    header_timeout: Duration,
    body_timeout: Duration,
) -> Result<(), HypervisorError> {
    fc_request(
        "PUT",
        socket_path,
        api_path,
        body,
        connect_timeout,
        status_timeout,
        header_timeout,
        body_timeout,
    )
    .await
}

pub async fn fc_patch_with_timeouts(
    socket_path: &str,
    api_path: &str,
    body: &str,
    connect_timeout: Duration,
    status_timeout: Duration,
    header_timeout: Duration,
    body_timeout: Duration,
) -> Result<(), HypervisorError> {
    fc_request(
        "PATCH",
        socket_path,
        api_path,
        body,
        connect_timeout,
        status_timeout,
        header_timeout,
        body_timeout,
    )
    .await
}

/// Poll until the Unix socket file appears (Firecracker is ready to accept API calls).
pub async fn wait_for_socket(path: &str, timeout: Duration) -> Result<(), HypervisorError> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if tokio::fs::metadata(path).await.is_ok() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(HypervisorError::SocketTimeout(path.to_string()));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    #[tokio::test]
    async fn response_parser_accepts_2xx_and_ignores_body() {
        let (client, mut server) = duplex(4096);
        let response = b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let response_writer = tokio::spawn(async move {
            server.write_all(response).await.expect("write response");
        });

        let mut reader = BufReader::new(client);
        let result = read_firecracker_response(
            &mut reader,
            "/actions",
            Duration::from_secs(2),
            Duration::from_secs(2),
            Duration::from_secs(2),
        )
        .await;
        let _ = response_writer.await;
        assert!(result.is_ok(), "{result:?}");
    }

    #[tokio::test]
    async fn response_parser_includes_response_body_on_error() {
        let response = b"HTTP/1.1 400 Bad Request\r\nContent-Length: 22\r\nConnection: close\r\n\r\ninstance not ready yet";
        let (client, mut server) = duplex(4096);
        let response_writer = tokio::spawn(async move {
            server.write_all(response).await.expect("write response");
        });

        let mut reader = BufReader::new(client);
        let result = read_firecracker_response(
            &mut reader,
            "/actions",
            Duration::from_secs(2),
            Duration::from_secs(2),
            Duration::from_secs(2),
        )
        .await;
        let _ = response_writer.await;

        let err = result.expect_err("expected api error");
        let msg = err.to_string();
        assert!(msg.contains("HTTP/1.1 400 Bad Request"), "{msg}");
        assert!(msg.contains("instance not ready yet"), "{msg}");
    }
}
