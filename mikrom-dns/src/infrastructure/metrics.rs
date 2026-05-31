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

use lazy_static::lazy_static;
use prometheus::{
    CounterVec, Encoder, Gauge, TextEncoder, opts, register_counter_vec, register_gauge,
};

lazy_static! {
    static ref DNS_QUERIES_TOTAL: CounterVec = register_counter_vec!(
        opts!("dns_queries_total", "Total number of DNS queries received"),
        &["zone", "query_type"]
    )
    .expect("Can't create dns_queries_total counter");
    static ref DNS_RESPONSE_CODE_TOTAL: CounterVec = register_counter_vec!(
        opts!(
            "dns_response_code_total",
            "Total number of DNS responses sent"
        ),
        &["rcode"]
    )
    .expect("Can't create dns_response_code_total counter");
    static ref DNS_ACTIVE_RECORDS: Gauge = register_gauge!(opts!(
        "dns_active_records",
        "Current number of active DNS records"
    ))
    .expect("Can't create dns_active_records gauge");
    static ref DNS_DROPPED_QUERIES_TOTAL: CounterVec = register_counter_vec!(
        opts!(
            "dns_dropped_queries_total",
            "Total number of dropped DNS queries"
        ),
        &["reason"]
    )
    .expect("Can't create dns_dropped_queries_total counter");
}

pub fn record_query(zone: &str, query_type: &str) {
    DNS_QUERIES_TOTAL
        .with_label_values(&[zone, query_type])
        .inc();
}

pub fn record_response(rcode: &str) {
    DNS_RESPONSE_CODE_TOTAL.with_label_values(&[rcode]).inc();
}

pub fn record_drop(reason: &str) {
    DNS_DROPPED_QUERIES_TOTAL.with_label_values(&[reason]).inc();
}

pub fn set_active_records(count: usize) {
    DNS_ACTIVE_RECORDS.set(count as f64);
}

pub fn render_metrics() -> String {
    let encoder = TextEncoder::new();
    let mut buffer = Vec::new();
    encoder
        .encode(&prometheus::gather(), &mut buffer)
        .expect("metrics encode failed");
    String::from_utf8(buffer).expect("metrics utf8 conversion failed")
}
