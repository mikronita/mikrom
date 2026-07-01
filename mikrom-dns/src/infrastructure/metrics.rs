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

use opentelemetry::global;
use opentelemetry::metrics::{Counter, Gauge};
use std::sync::OnceLock;

struct DnsOtelMetrics {
    dns_queries_total: Counter<u64>,
    dns_response_code_total: Counter<u64>,
    dns_dropped_queries_total: Counter<u64>,
    dns_active_records: Gauge<u64>,
}

impl DnsOtelMetrics {
    fn get() -> &'static Self {
        static METRICS: OnceLock<DnsOtelMetrics> = OnceLock::new();
        METRICS.get_or_init(|| {
            let meter = global::meter("mikrom-dns");
            Self {
                dns_queries_total: meter.u64_counter("dns_queries_total").build(),
                dns_response_code_total: meter.u64_counter("dns_response_code_total").build(),
                dns_dropped_queries_total: meter.u64_counter("dns_dropped_queries_total").build(),
                dns_active_records: meter.u64_gauge("dns_active_records").build(),
            }
        })
    }
}

pub fn record_query(zone: &str, query_type: &str) {
    let attrs = [
        opentelemetry::KeyValue::new("zone", zone.to_string()),
        opentelemetry::KeyValue::new("query_type", query_type.to_string()),
    ];
    DnsOtelMetrics::get().dns_queries_total.add(1, &attrs);
}

pub fn record_response(rcode: &str) {
    let attrs = [opentelemetry::KeyValue::new("rcode", rcode.to_string())];
    DnsOtelMetrics::get().dns_response_code_total.add(1, &attrs);
}

pub fn record_drop(reason: &str) {
    let attrs = [opentelemetry::KeyValue::new("reason", reason.to_string())];
    DnsOtelMetrics::get()
        .dns_dropped_queries_total
        .add(1, &attrs);
}

pub fn set_active_records(count: usize) {
    DnsOtelMetrics::get()
        .dns_active_records
        .record(count as u64, &[]);
}
