#[cfg(test)]
mod tests {
    use crate::proxy::{MikromProxy, RouterMetricsCounters};
    use crate::state::{Route, State};
    use pingora::lb::LoadBalancer;
    use pingora::lb::selection::RoundRobin;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn test_proxy_load_balancing() {
        let mut routes = HashMap::new();
        let targets = vec!["[fd00::1]:8080".to_string(), "[fd00::2]:8080".to_string()];
        let lb = LoadBalancer::<RoundRobin>::try_from_iter(targets.as_slice()).unwrap();

        routes.insert(
            "app.mikrom.local".to_string(),
            Route {
                host: "app.mikrom.local".to_string(),
                targets: targets.clone(),
                lb: Arc::new(lb),
                use_tls: false,
            },
        );

        let state = Arc::new(RwLock::new(State {
            routes,
            acme_tokens: HashMap::new(),
            certificates: HashMap::new(),
        }));

        let metrics = Arc::new(RouterMetricsCounters::new());
        let proxy = MikromProxy::new(state, false, metrics, 100);

        let lb = proxy.get_lb("app.mikrom.local").await.unwrap();
        let t1 = lb.select(b"", 256).unwrap();
        let t2 = lb.select(b"", 256).unwrap();

        // Check if both targets are selected (order might vary but they should be there)
        let t1_str = t1.to_string();
        let t2_str = t2.to_string();
        assert!(targets.contains(&t1_str));
        assert!(targets.contains(&t2_str));
        assert_ne!(t1_str, t2_str);
    }

    #[tokio::test]
    async fn test_round_robin_rotation() {
        let mut routes = HashMap::new();
        let targets = vec!["10.0.0.1:8080".to_string(), "10.0.0.2:8080".to_string()];
        let lb = LoadBalancer::<RoundRobin>::try_from_iter(targets.as_slice()).unwrap();

        routes.insert(
            "app.mikrom.local".to_string(),
            Route {
                host: "app.mikrom.local".to_string(),
                targets: targets.clone(),
                lb: Arc::new(lb),
                use_tls: false,
            },
        );

        let state = Arc::new(RwLock::new(State {
            routes,
            acme_tokens: HashMap::new(),
            certificates: HashMap::new(),
        }));

        let metrics = Arc::new(RouterMetricsCounters::new());
        let proxy = MikromProxy::new(state, false, metrics, 100);

        let lb = proxy.get_lb("app.mikrom.local").await.unwrap();
        let t1 = lb.select(b"", 256).unwrap().to_string();
        let t2 = lb.select(b"", 256).unwrap().to_string();
        let t3 = lb.select(b"", 256).unwrap().to_string();

        assert_ne!(t1, t2);
        assert_eq!(t1, t3);
    }
}
