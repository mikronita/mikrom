#[allow(unreachable_pub)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HostName(String);

impl HostName {
    #[allow(unreachable_pub)]
    pub fn parse(host: &str) -> Self {
        Self(canonical_host(host))
    }

    #[allow(unreachable_pub)]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for HostName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

pub(super) fn canonical_host(host: &str) -> String {
    if let Some(rest) = host.strip_prefix('[')
        && let Some((ipv6, suffix)) = rest.split_once(']')
        && (suffix.is_empty() || suffix.starts_with(':'))
    {
        return format!("[{ipv6}]");
    }

    if let Some((name, port)) = host.rsplit_once(':')
        && !name.contains(':')
        && !port.contains(':')
    {
        return name.to_string();
    }

    host.to_string()
}

#[cfg(test)]
mod tests {
    use super::{HostName, canonical_host};

    #[test]
    fn strips_port_from_ipv4_or_hostname() {
        assert_eq!(canonical_host("example.com:443"), "example.com");
        assert_eq!(canonical_host("app.internal:8080"), "app.internal");
    }

    #[test]
    fn preserves_ipv6_literal_without_port() {
        assert_eq!(canonical_host("[fd00::1]:443"), "[fd00::1]");
        assert_eq!(canonical_host("[fd00::1]"), "[fd00::1]");
    }

    #[test]
    fn host_name_parse_normalizes_once() {
        let host = HostName::parse("registry.example.com:443");
        assert_eq!(host.as_str(), "registry.example.com");
    }
}
