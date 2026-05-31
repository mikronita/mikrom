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
use tokio::net::TcpStream;
use tokio::time::{Duration, timeout};
use tracing::info;

pub struct UpstreamDnsForwarder {
    upstreams: Vec<SocketAddr>,
}

impl UpstreamDnsForwarder {
    pub async fn connect(upstream_dns: &[SocketAddr]) -> Result<Self> {
        for upstream in upstream_dns {
            info!(%upstream, "Configured upstream DNS");
        }

        Ok(Self {
            upstreams: upstream_dns.to_vec(),
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

        let mut stream = timeout(Duration::from_secs(5), TcpStream::connect(upstream))
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
        timeout(Duration::from_secs(5), stream.read_exact(&mut len_buf))
            .await
            .context("timeout reading upstream response length")?
            .context("failed to read upstream response length")?;
        let response_len = usize::from(u16::from_be_bytes(len_buf));
        let mut response_buf = vec![0u8; response_len];
        timeout(Duration::from_secs(5), stream.read_exact(&mut response_buf))
            .await
            .context("timeout reading upstream response body")?
            .context("failed to read upstream response body")?;

        let mut message =
            Message::from_vec(&response_buf).context("failed to decode upstream DNS response")?;
        if message.metadata.response_code == ResponseCode::NotImp {
            message.metadata.response_code = ResponseCode::NoError;
        }

        Ok(message)
    }
}
