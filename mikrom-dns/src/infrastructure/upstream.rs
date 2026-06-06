#![allow(
    clippy::cast_precision_loss,
    clippy::let_and_return,
    clippy::manual_let_else,
    clippy::missing_const_for_fn,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::non_std_lazy_statics,
    clippy::single_match_else,
    clippy::struct_field_names,
    clippy::suboptimal_flops,
    clippy::unchecked_time_subtraction,
    clippy::unused_async
)]

use anyhow::{Context, Result};
use hickory_server::proto::op::{Message, MessageType, OpCode, Query, ResponseCode};
use hickory_server::proto::rr::{DNSClass, Name, RecordType};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::time::{Duration, timeout};
use tracing::info;

pub struct UpstreamDnsForwarder {
    upstreams: Vec<SocketAddr>,
    timeout: Duration,
}

impl UpstreamDnsForwarder {
    pub async fn connect(upstream_dns: &[SocketAddr], timeout: Duration) -> Result<Self> {
        for upstream in upstream_dns {
            info!(%upstream, "Configured upstream DNS");
        }

        Ok(Self {
            upstreams: upstream_dns.to_vec(),
            timeout,
        })
    }

    pub async fn forward(&self, name: &Name, query_type: RecordType) -> Result<Message> {
        let mut last_error: Option<anyhow::Error> = None;

        for upstream in &self.upstreams {
            match self.query_single(*upstream, name, query_type).await {
                Ok(message) => return Ok(message),
                Err(err) => {
                    last_error = Some(err);
                },
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("upstream DNS query failed")))
    }

    async fn query_single(
        &self,
        upstream: SocketAddr,
        name: &Name,
        query_type: RecordType,
    ) -> Result<Message> {
        let mut request = Message::new(0, MessageType::Query, OpCode::Query);
        request.metadata.recursion_desired = true;
        let mut query = Query::query(name.clone(), query_type);
        query.set_query_class(DNSClass::IN);
        request.add_query(query);

        let request_bytes = request
            .to_vec()
            .context("failed to encode upstream DNS request")?;

        match self.query_via_udp(upstream, &request_bytes).await {
            Ok(message) if !message.metadata.truncation => {
                return Ok(Self::normalize_response(message));
            },
            Ok(_) | Err(_) => {},
        }

        self.query_via_tcp(upstream, &request_bytes).await
    }

    async fn query_via_udp(&self, upstream: SocketAddr, request_bytes: &[u8]) -> Result<Message> {
        let bind_addr = "[::]:0";
        let socket = timeout(self.timeout, UdpSocket::bind(bind_addr))
            .await
            .context("timeout binding UDP socket")?
            .context("failed to bind UDP socket")?;

        timeout(self.timeout, socket.send_to(request_bytes, upstream))
            .await
            .context("timeout sending upstream UDP request")?
            .context("failed to send upstream UDP request")?;

        let mut response_buf = vec![0u8; 4096];
        let (response_len, _) = timeout(self.timeout, socket.recv_from(&mut response_buf))
            .await
            .context("timeout reading upstream UDP response")?
            .context("failed to read upstream UDP response")?;

        response_buf.truncate(response_len);
        let message =
            Message::from_vec(&response_buf).context("failed to decode upstream UDP response")?;

        Ok(message)
    }

    async fn query_via_tcp(&self, upstream: SocketAddr, request_bytes: &[u8]) -> Result<Message> {
        let mut stream = timeout(self.timeout, TcpStream::connect(upstream))
            .await
            .context("timeout connecting to upstream DNS")?
            .context("failed to connect to upstream DNS")?;

        let request_len = u16::try_from(request_bytes.len())
            .context("upstream DNS request too large")?
            .to_be_bytes();
        stream
            .write_all(&request_len)
            .await
            .context("failed to write upstream request length")?;
        stream
            .write_all(&request_bytes)
            .await
            .context("failed to write upstream request body")?;
        stream
            .flush()
            .await
            .context("failed to flush upstream request")?;

        let mut len_buf = [0u8; 2];
        timeout(self.timeout, stream.read_exact(&mut len_buf))
            .await
            .context("timeout reading upstream response length")?
            .context("failed to read upstream response length")?;
        let response_len = usize::from(u16::from_be_bytes(len_buf));
        let mut response_buf = vec![0u8; response_len];
        timeout(self.timeout, stream.read_exact(&mut response_buf))
            .await
            .context("timeout reading upstream response body")?
            .context("failed to read upstream response body")?;

        let message =
            Message::from_vec(&response_buf).context("failed to decode upstream DNS response")?;
        Ok(Self::normalize_response(message))
    }

    fn normalize_response(mut message: Message) -> Message {
        if message.metadata.response_code == ResponseCode::NotImp {
            message.metadata.response_code = ResponseCode::NoError;
        }

        message
    }
}
