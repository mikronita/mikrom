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

use crate::domain::MikromZone;
use dashmap::DashMap;
use std::net::Ipv6Addr;
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct DnsRecordStore {
    user_records: Arc<DashMap<String, Ipv6Addr>>,
    net_records: Arc<DashMap<String, Ipv6Addr>>,
    sys_records: Arc<DashMap<String, Ipv6Addr>>,
}

impl DnsRecordStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn user_records(&self) -> Arc<DashMap<String, Ipv6Addr>> {
        Arc::clone(&self.user_records)
    }

    pub fn network_records(&self) -> Arc<DashMap<String, Ipv6Addr>> {
        Arc::clone(&self.net_records)
    }

    pub fn system_records(&self) -> Arc<DashMap<String, Ipv6Addr>> {
        Arc::clone(&self.sys_records)
    }

    pub fn insert_user(&self, key: impl Into<String>, ip: Ipv6Addr) {
        self.user_records.insert(key.into(), ip);
    }

    pub fn remove_user(&self, key: &str) {
        self.user_records.remove(key);
    }

    pub fn insert_network(&self, key: impl Into<String>, ip: Ipv6Addr) {
        self.net_records.insert(key.into(), ip);
    }

    pub fn insert_system(&self, key: impl Into<String>, ip: Ipv6Addr) {
        self.sys_records.insert(key.into(), ip);
    }

    pub fn get(&self, zone: MikromZone, key: &str) -> Option<Ipv6Addr> {
        match zone {
            MikromZone::System => self.sys_records.get(key).map(|record| *record),
            MikromZone::Network => self.net_records.get(key).map(|record| *record),
            MikromZone::User => self.user_records.get(key).map(|record| *record),
            MikromZone::External => None,
        }
    }

    pub fn active_records(&self) -> usize {
        self.user_records.len() + self.net_records.len() + self.sys_records.len()
    }

    pub fn contains_user(&self, key: &str) -> bool {
        self.user_records.contains_key(key)
    }

    pub fn contains_network(&self, key: &str) -> bool {
        self.net_records.contains_key(key)
    }
}
