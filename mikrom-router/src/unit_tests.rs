use crate::proxy::RouterMetricsCounters;
use std::sync::atomic::Ordering;

#[test]
fn test_metrics_counters() {
    let metrics = RouterMetricsCounters::new();
    assert_eq!(metrics.requests_total.load(Ordering::Relaxed), 0);

    metrics.requests_total.fetch_add(1, Ordering::Relaxed);
    metrics.responses_2xx.fetch_add(1, Ordering::Relaxed);

    assert_eq!(metrics.requests_total.load(Ordering::Relaxed), 1);
    assert_eq!(metrics.responses_2xx.load(Ordering::Relaxed), 1);
    assert_eq!(metrics.responses_4xx.load(Ordering::Relaxed), 0);
}

#[cfg(test)]
mod ipv6_helpers {
    use crate::state::Route;
    use pingora::lb::LoadBalancer;
    use pingora::lb::selection::RoundRobin;
    use std::net::{IpAddr, Ipv6Addr};
    use std::sync::Arc;

    #[test]
    fn test_ipv6_string_normalization() {
        let ip = IpAddr::V6(Ipv6Addr::LOCALHOST);
        assert_eq!(ip.to_string(), "::1");

        let ip_long = IpAddr::V6(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1));
        assert_eq!(ip_long.to_string(), "fd00::1");
    }

    #[test]
    fn test_ipv6_load_balancer_selection() {
        let targets = vec!["[fd00::1]:80".to_string(), "[fd00::2]:80".to_string()];
        let lb = LoadBalancer::<RoundRobin>::try_from_iter(targets.as_slice()).unwrap();
        let route = Route {
            host: "test.local".to_string(),
            targets: targets.clone(),
            lb: Arc::new(lb),
            use_tls: false,
        };

        let upstream1 = route.lb.select(b"", 256).unwrap();
        let upstream2 = route.lb.select(b"", 256).unwrap();

        assert!(targets.contains(&upstream1.to_string()));
        assert!(targets.contains(&upstream2.to_string()));
        assert_ne!(upstream1.to_string(), upstream2.to_string());
    }
}
