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

use crate::application::resolution::{DnsResolutionService, ResolutionDecision};
use crate::domain::MikromZone;
use crate::infrastructure::metrics;
use crate::infrastructure::upstream::UpstreamDnsForwarder;
use anyhow::Context;
use async_trait::async_trait;
use hickory_server::net::runtime::Time;
use hickory_server::proto::op::{Header, HeaderCounts, Metadata, ResponseCode};
use hickory_server::proto::rr::rdata::AAAA;
use hickory_server::proto::rr::{RData, Record, RecordType};
use hickory_server::server::{Request, RequestHandler, ResponseHandler, ResponseInfo};
use hickory_server::zone_handler::MessageResponseBuilder;
use std::sync::Arc;
use tracing::warn;

pub struct MikromDnsHandler {
    resolver: Arc<DnsResolutionService>,
    upstream: Option<Arc<UpstreamDnsForwarder>>,
}

impl MikromDnsHandler {
    pub fn new(resolver: DnsResolutionService, upstream: Option<UpstreamDnsForwarder>) -> Self {
        Self {
            resolver: Arc::new(resolver),
            upstream: upstream.map(Arc::new),
        }
    }

    async fn send_empty_response<R: ResponseHandler>(
        &self,
        metadata: Metadata,
        rcode: ResponseCode,
        request: &Request,
        mut response_handle: R,
    ) -> ResponseInfo {
        let mut metadata = metadata;
        metadata.response_code = rcode;
        let response =
            MessageResponseBuilder::from_message_request(request).build_no_records(metadata);
        response_handle
            .send_response(response)
            .await
            .unwrap_or_else(|_| {
                ResponseInfo::from(Header {
                    metadata,
                    counts: HeaderCounts::default(),
                })
            })
    }
}

#[async_trait]
impl RequestHandler for MikromDnsHandler {
    async fn handle_request<R: ResponseHandler, T: Time>(
        &self,
        request: &Request,
        mut response_handle: R,
    ) -> ResponseInfo {
        let src_ip = request.src().ip();
        let query = match request.queries.queries().first() {
            Some(query) => query,
            None => {
                let metadata = Metadata::response_from_request(&request.metadata);
                warn!(%src_ip, "DNS request without queries");
                metrics::record_response(ResponseCode::ServFail.to_string().as_str());
                return self
                    .send_empty_response(metadata, ResponseCode::ServFail, request, response_handle)
                    .await;
            },
        };

        let mut metadata = Metadata::response_from_request(&request.metadata);
        metadata.authoritative = true;

        let name = query.name().clone();
        let query_type = query.query_type();
        let zone = MikromZone::from_name(&name);
        tracing::info!(%src_ip, %name, %query_type, zone = zone.as_str(), "DNS query received");
        metrics::record_query(zone.as_str(), &query_type.to_string());

        let decision = self.resolver.resolve(src_ip, &name, query_type);
        let response_info = match decision {
            ResolutionDecision::Empty(rcode) => {
                if zone == MikromZone::External && query_type == RecordType::AAAA {
                    warn!(%src_ip, ?name, ?query_type, "External query resolved without local record");
                }
                metrics::record_response(rcode.to_string().as_str());
                self.send_empty_response(metadata, rcode, request, response_handle)
                    .await
            },
            ResolutionDecision::Dropped {
                response_code,
                reason,
            } => {
                metrics::record_drop(reason);
                metrics::record_response(response_code.to_string().as_str());
                self.send_empty_response(metadata, response_code, request, response_handle)
                    .await
            },
            ResolutionDecision::ForwardUpstream => {
                match self.forward_upstream(&name, query_type).await {
                    Ok(message) => {
                        self.send_forwarded_response(request, metadata, response_handle, message)
                            .await
                    },
                    Err(err) => {
                        warn!(%src_ip, error = %err, ?name, ?query_type, "Upstream DNS lookup failed");
                        metrics::record_drop("upstream");
                        metrics::record_response(ResponseCode::ServFail.to_string().as_str());
                        self.send_empty_response(
                            metadata,
                            ResponseCode::ServFail,
                            request,
                            response_handle,
                        )
                        .await
                    },
                }
            },
            ResolutionDecision::Aaaa { address, ttl } => {
                let record =
                    Record::from_rdata(name.clone().into(), ttl, RData::AAAA(AAAA(address)));
                let response = MessageResponseBuilder::from_message_request(request).build(
                    metadata,
                    [&record],
                    vec![],
                    vec![],
                    vec![],
                );
                let response_info = response_handle
                    .send_response(response)
                    .await
                    .unwrap_or_else(|_| {
                        ResponseInfo::from(Header {
                            metadata,
                            counts: HeaderCounts::default(),
                        })
                    });
                metrics::record_response(response_info.response_code.to_string().as_str());
                response_info
            },
        };

        response_info
    }
}

impl MikromDnsHandler {
    async fn forward_upstream(
        &self,
        name: &hickory_server::proto::rr::Name,
        query_type: RecordType,
    ) -> anyhow::Result<hickory_server::proto::op::Message> {
        let upstream = self
            .upstream
            .as_ref()
            .context("upstream DNS is not configured")?;
        upstream.forward(name, query_type).await
    }

    async fn send_forwarded_response<R: ResponseHandler>(
        &self,
        request: &Request,
        mut metadata: Metadata,
        mut response_handle: R,
        message: hickory_server::proto::op::Message,
    ) -> ResponseInfo {
        metadata.response_code = message.metadata.response_code;
        metadata.authoritative = message.metadata.authoritative;
        metadata.recursion_available = message.metadata.recursion_available;
        metadata.truncation = message.metadata.truncation;

        let response = MessageResponseBuilder::from_message_request(request).build(
            metadata,
            &message.answers,
            &message.authorities,
            std::iter::empty::<&Record>(),
            &message.additionals,
        );
        response_handle
            .send_response(response)
            .await
            .unwrap_or_else(|_| {
                ResponseInfo::from(Header {
                    metadata,
                    counts: HeaderCounts::default(),
                })
            })
    }
}
