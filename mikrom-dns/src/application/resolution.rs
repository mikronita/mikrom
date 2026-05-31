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

use crate::application::records::DnsRecordStore;
use crate::domain::{MikromZone, TokenBucket, USER_RECORD_TTL, extract_record_key};
use hickory_server::proto::op::ResponseCode;
use hickory_server::proto::rr::{Name, RecordType};
use std::net::{IpAddr, Ipv6Addr, SocketAddr};

pub enum ResolutionDecision {
    Empty(ResponseCode),
    Dropped {
        response_code: ResponseCode,
        reason: &'static str,
    },
    ForwardUpstream,
    Aaaa {
        address: Ipv6Addr,
        ttl: u32,
    },
}

pub struct DnsResolutionService {
    store: DnsRecordStore,
    upstream_dns: Vec<SocketAddr>,
    allowed_subnets: Vec<ipnet::IpNet>,
    rate_limit_map: dashmap::DashMap<IpAddr, TokenBucket>,
    rate_limit_qps: f64,
    rate_limit_burst: f64,
}

impl DnsResolutionService {
    pub fn new(
        store: DnsRecordStore,
        upstream_dns: Vec<SocketAddr>,
        allowed_subnets: Vec<ipnet::IpNet>,
    ) -> Self {
        Self::with_limits(store, upstream_dns, allowed_subnets, 100.0, 200.0)
    }

    pub fn with_limits(
        store: DnsRecordStore,
        upstream_dns: Vec<SocketAddr>,
        allowed_subnets: Vec<ipnet::IpNet>,
        rate_limit_qps: f64,
        rate_limit_burst: f64,
    ) -> Self {
        Self {
            store,
            upstream_dns,
            allowed_subnets,
            rate_limit_map: dashmap::DashMap::new(),
            rate_limit_qps,
            rate_limit_burst,
        }
    }

    pub fn resolve(
        &self,
        source_ip: IpAddr,
        name: &Name,
        query_type: RecordType,
    ) -> ResolutionDecision {
        if !self.allowed_subnets.is_empty()
            && !self
                .allowed_subnets
                .iter()
                .any(|net| net.contains(&source_ip))
        {
            return ResolutionDecision::Dropped {
                response_code: ResponseCode::Refused,
                reason: "acl",
            };
        }

        let mut bucket = self
            .rate_limit_map
            .entry(source_ip)
            .or_insert_with(|| TokenBucket::new(self.rate_limit_qps));
        if !bucket.check(self.rate_limit_qps, self.rate_limit_burst) {
            return ResolutionDecision::Dropped {
                response_code: ResponseCode::ServFail,
                reason: "rate_limit",
            };
        }
        drop(bucket);

        let zone = MikromZone::from_name(name);
        if query_type == RecordType::AAAA {
            if let Some(address) =
                extract_record_key(name, zone).and_then(|key| self.store.get(zone, &key))
            {
                return ResolutionDecision::Aaaa {
                    address,
                    ttl: USER_RECORD_TTL,
                };
            }

            if zone == MikromZone::External && !self.upstream_dns.is_empty() {
                return ResolutionDecision::ForwardUpstream;
            }

            return ResolutionDecision::Empty(ResponseCode::NXDomain);
        }

        if zone == MikromZone::External && self.upstream_dns.is_empty() {
            ResolutionDecision::Empty(ResponseCode::NXDomain)
        } else if zone == MikromZone::External {
            ResolutionDecision::ForwardUpstream
        } else {
            ResolutionDecision::Empty(ResponseCode::NoError)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv6Addr;

    #[test]
    fn resolves_internal_aaaa_records() {
        let store = DnsRecordStore::new();
        store.insert_user(
            "app.tenant",
            "fdac:5111:a310:e0bd::1"
                .parse::<Ipv6Addr>()
                .expect("valid ipv6"),
        );
        let resolver = DnsResolutionService::with_limits(store, vec![], vec![], 10.0, 10.0);

        let decision = resolver.resolve(
            "127.0.0.1".parse().expect("valid ip"),
            &Name::from_ascii("app.tenant.u.mikrom.internal.").expect("valid name"),
            RecordType::AAAA,
        );

        match decision {
            ResolutionDecision::Aaaa { address, ttl } => {
                assert_eq!(
                    address,
                    "fdac:5111:a310:e0bd::1"
                        .parse::<Ipv6Addr>()
                        .expect("valid ipv6")
                );
                assert_eq!(ttl, USER_RECORD_TTL);
            },
            _ => panic!("expected AAAA answer"),
        }
    }

    #[test]
    fn returns_no_error_for_non_aaaa_internal_queries() {
        let resolver =
            DnsResolutionService::with_limits(DnsRecordStore::new(), vec![], vec![], 10.0, 10.0);

        match resolver.resolve(
            "127.0.0.1".parse().expect("valid ip"),
            &Name::from_ascii("api.s.mikrom.internal.").expect("valid name"),
            RecordType::A,
        ) {
            ResolutionDecision::Empty(ResponseCode::NoError) => {},
            _ => panic!("expected NoError"),
        }
    }

    #[test]
    fn refuses_acl_violations() {
        let resolver = DnsResolutionService::with_limits(
            DnsRecordStore::new(),
            vec![],
            vec!["fd00::/64".parse().expect("valid subnet")],
            10.0,
            10.0,
        );

        match resolver.resolve(
            "::1".parse().expect("valid ip"),
            &Name::from_ascii("api.s.mikrom.internal.").expect("valid name"),
            RecordType::A,
        ) {
            ResolutionDecision::Dropped {
                response_code: ResponseCode::Refused,
                reason: "acl",
            } => {},
            _ => panic!("expected Refused"),
        }
    }

    #[test]
    fn rate_limits_after_burst_exhaustion() {
        let resolver =
            DnsResolutionService::with_limits(DnsRecordStore::new(), vec![], vec![], 1.0, 1.0);

        let name = Name::from_ascii("api.s.mikrom.internal.").expect("valid name");
        assert!(matches!(
            resolver.resolve("127.0.0.1".parse().expect("valid ip"), &name, RecordType::A),
            ResolutionDecision::Empty(ResponseCode::NoError)
        ));
        assert!(matches!(
            resolver.resolve("127.0.0.1".parse().expect("valid ip"), &name, RecordType::A),
            ResolutionDecision::Dropped {
                response_code: ResponseCode::ServFail,
                reason: "rate_limit"
            }
        ));
    }
}
