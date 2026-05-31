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

use hickory_server::proto::rr::Name;

pub const ROOT_ZONE: &str = "mikrom.internal.";
pub const SYS_ZONE: &str = "s.mikrom.internal.";
pub const NET_ZONE: &str = "n.mikrom.internal.";
pub const USER_ZONE: &str = "u.mikrom.internal.";
pub const USER_RECORD_TTL: u32 = 5;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum MikromZone {
    System,
    Network,
    User,
    External,
}

impl MikromZone {
    pub fn from_name(name: &Name) -> Self {
        let name_str = name.to_lowercase().to_string();
        if name_str.ends_with(SYS_ZONE) {
            Self::System
        } else if name_str.ends_with(NET_ZONE) {
            Self::Network
        } else if name_str.ends_with(USER_ZONE) {
            Self::User
        } else {
            Self::External
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "sys",
            Self::Network => "net",
            Self::User => "user",
            Self::External => "external",
        }
    }

    pub fn suffix(self) -> Option<&'static str> {
        match self {
            Self::System => Some(SYS_ZONE),
            Self::Network => Some(NET_ZONE),
            Self::User => Some(USER_ZONE),
            Self::External => None,
        }
    }
}

pub fn extract_record_key(name: &Name, zone: MikromZone) -> Option<String> {
    let suffix = zone.suffix()?;
    let lowered = name.to_lowercase().to_string();
    let stripped = lowered.strip_suffix(suffix)?.trim_end_matches('.');
    if stripped.is_empty() {
        None
    } else {
        Some(stripped.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_internal_zones() {
        assert_eq!(
            MikromZone::from_name(&Name::from_ascii("api.s.mikrom.internal.").expect("valid name")),
            MikromZone::System
        );
        assert_eq!(
            MikromZone::from_name(
                &Name::from_ascii("worker-01.n.mikrom.internal.").expect("valid name")
            ),
            MikromZone::Network
        );
        assert_eq!(
            MikromZone::from_name(
                &Name::from_ascii("customer-db.tenant.u.mikrom.internal.").expect("valid name")
            ),
            MikromZone::User
        );
        assert_eq!(
            MikromZone::from_name(&Name::from_ascii("example.com.").expect("valid name")),
            MikromZone::External
        );
    }

    #[test]
    fn extracts_record_keys_from_fqdns() {
        assert_eq!(
            extract_record_key(
                &Name::from_ascii("api.s.mikrom.internal.").expect("valid name"),
                MikromZone::System
            ),
            Some("api".to_string())
        );
        assert_eq!(
            extract_record_key(
                &Name::from_ascii("worker-01.n.mikrom.internal.").expect("valid name"),
                MikromZone::Network
            ),
            Some("worker-01".to_string())
        );
        assert_eq!(
            extract_record_key(
                &Name::from_ascii("customer-db.tenant.u.mikrom.internal.").expect("valid name"),
                MikromZone::User
            ),
            Some("customer-db.tenant".to_string())
        );
        assert_eq!(
            extract_record_key(
                &Name::from_ascii("example.com.").expect("valid name"),
                MikromZone::External
            ),
            None
        );
    }
}
