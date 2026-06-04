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
use hickory_server::proto::op::{Header, HeaderCounts, Message, Metadata, ResponseCode};
use hickory_server::proto::rr::rdata::{A, AAAA};
use hickory_server::proto::rr::{RData, Record, RecordType};
use hickory_server::server::{Request, RequestHandler, ResponseHandler, ResponseInfo};
use hickory_server::zone_handler::MessageResponseBuilder;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use tracing::warn;

pub struct MikromDnsHandler {
    resolver: Arc<DnsResolutionService>,
    upstream: Option<Arc<UpstreamDnsForwarder>>,
}

enum Dns64Resolution {
    Forwarded(Message),
    Synthesized {
        metadata: Metadata,
        answers: Vec<Record>,
        authorities: Vec<Record>,
        additionals: Vec<Record>,
    },
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::records::DnsRecordStore;
    use crate::application::resolution::DnsResolutionService;
    use std::net::Ipv6Addr;

    fn handler() -> MikromDnsHandler {
        let resolver = DnsResolutionService::with_limits(
            DnsRecordStore::new(),
            vec![],
            vec![],
            Ipv6Addr::new(0x0064, 0xff9b, 0, 0, 0, 0, 0, 0),
            10.0,
            10.0,
        );
        MikromDnsHandler::new(resolver, None)
    }

    #[test]
    fn synthesizes_nat64_addresses_from_ipv4_records() {
        let handler = handler();
        let record = Record::from_rdata(
            hickory_server::proto::rr::Name::from_ascii("example.com.").expect("valid name"),
            30,
            RData::A(A("203.0.113.10".parse().expect("valid ipv4"))),
        );

        let synthesized = handler.synthesize_dns64_record(&record);
        match synthesized.data {
            RData::AAAA(AAAA(addr)) => {
                assert_eq!(
                    addr,
                    "64:ff9b::cb00:710a"
                        .parse::<Ipv6Addr>()
                        .expect("valid ipv6")
                );
            },
            other => panic!("unexpected synthesized record: {other:?}"),
        }
    }
}

#[allow(clippy::too_many_lines)]
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
                if zone == MikromZone::External && query_type == RecordType::AAAA {
                    match self.resolve_dns64_aaaa(request, &name).await {
                        Ok(Dns64Resolution::Forwarded(message)) => {
                            let response_info = self
                                .send_forwarded_response(
                                    request,
                                    metadata,
                                    response_handle,
                                    message,
                                )
                                .await;
                            metrics::record_response(
                                response_info.response_code.to_string().as_str(),
                            );
                            return response_info;
                        },
                        Ok(Dns64Resolution::Synthesized {
                            metadata: response_metadata,
                            answers,
                            authorities,
                            additionals,
                        }) => {
                            let response = MessageResponseBuilder::from_message_request(request)
                                .build(
                                    response_metadata,
                                    &answers,
                                    &authorities,
                                    std::iter::empty::<&Record>(),
                                    &additionals,
                                );
                            let response_info = response_handle
                                .send_response(response)
                                .await
                                .unwrap_or_else(|_| {
                                    ResponseInfo::from(Header {
                                        metadata: response_metadata,
                                        counts: HeaderCounts::default(),
                                    })
                                });
                            metrics::record_response(
                                response_info.response_code.to_string().as_str(),
                            );
                            return response_info;
                        },
                        Err(err) => {
                            warn!(%src_ip, error = %err, ?name, ?query_type, "DNS64 resolution failed");
                            metrics::record_drop("dns64");
                            metrics::record_response(ResponseCode::ServFail.to_string().as_str());
                            return self
                                .send_empty_response(
                                    Metadata::response_from_request(&request.metadata),
                                    ResponseCode::ServFail,
                                    request,
                                    response_handle,
                                )
                                .await;
                        },
                    }
                }

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
    async fn resolve_dns64_aaaa(
        &self,
        request: &Request,
        name: &hickory_server::proto::rr::Name,
    ) -> anyhow::Result<Dns64Resolution> {
        let aaaa_message = self.forward_upstream(name, RecordType::AAAA).await?;
        if Self::message_has_aaaa_answers(&aaaa_message)
            || Self::should_preserve_upstream_empty(&aaaa_message)
        {
            return Ok(Dns64Resolution::Forwarded(aaaa_message));
        }

        let a_message = self.forward_upstream(name, RecordType::A).await?;
        let synthesized_answers = self.synthesize_dns64_answers(&a_message);
        if synthesized_answers.is_empty() {
            return Ok(Dns64Resolution::Forwarded(aaaa_message));
        }

        let mut response_metadata = Metadata::response_from_request(&request.metadata);
        response_metadata.response_code = a_message.metadata.response_code;
        response_metadata.authoritative = a_message.metadata.authoritative;
        response_metadata.recursion_available = a_message.metadata.recursion_available;
        response_metadata.truncation = a_message.metadata.truncation;
        Ok(Dns64Resolution::Synthesized {
            metadata: response_metadata,
            answers: synthesized_answers,
            authorities: a_message.authorities,
            additionals: a_message.additionals,
        })
    }

    fn message_has_aaaa_answers(message: &hickory_server::proto::op::Message) -> bool {
        message
            .answers
            .iter()
            .any(|record| matches!(&record.data, RData::AAAA(_)))
    }

    fn should_preserve_upstream_empty(message: &hickory_server::proto::op::Message) -> bool {
        matches!(
            message.metadata.response_code,
            ResponseCode::NXDomain | ResponseCode::ServFail
        )
    }

    fn synthesize_dns64_answers(
        &self,
        message: &hickory_server::proto::op::Message,
    ) -> Vec<Record> {
        message
            .answers
            .iter()
            .map(|record| self.synthesize_dns64_record(record))
            .collect()
    }

    fn synthesize_dns64_record(&self, record: &Record) -> Record {
        match &record.data {
            RData::A(A(ipv4)) => Record::from_rdata(
                record.name.clone(),
                record.ttl,
                RData::AAAA(AAAA(self.synthesize_nat64_ipv6(*ipv4))),
            ),
            _ => record.clone(),
        }
    }

    fn synthesize_nat64_ipv6(&self, ipv4: Ipv4Addr) -> Ipv6Addr {
        let prefix = self.resolver.nat64_prefix();
        let mut octets = prefix.octets();
        octets[12..16].copy_from_slice(&ipv4.octets());
        Ipv6Addr::from(octets)
    }

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
