#[cfg(test)]
mod tests {
    use crate::proxy::{MikromProxy, RouterMetricsCounters};
    use crate::state::{Route, State};
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn test_proxy_load_balancing() {
        let mut routes = HashMap::new();
        routes.insert(
            "app.mikrom.local".to_string(),
            Route {
                host: "app.mikrom.local".to_string(),
                targets: vec!["[fd00::1]:8080".to_string(), "[fd00::2]:8080".to_string()],
            },
        );

        let state = Arc::new(RwLock::new(State {
            routes,
            acme_tokens: HashMap::new(),
            certificates: HashMap::new(),
        }));

        let metrics = Arc::new(RouterMetricsCounters::new());
        let _proxy = MikromProxy::new(state, false, metrics);
    }

    #[tokio::test]
    async fn test_acme_challenge_interception_config() {
        let mut acme_tokens = HashMap::new();
        acme_tokens.insert(
            "example.com".to_string(),
            "challenge-response-123".to_string(),
        );

        let state = Arc::new(RwLock::new(State {
            routes: HashMap::new(),
            acme_tokens,
            certificates: HashMap::new(),
        }));

        let metrics = Arc::new(RouterMetricsCounters::new());
        let _proxy = MikromProxy::new(state, true, metrics);
    }

    #[tokio::test]
    async fn test_round_robin_rotation() {
        let mut routes = HashMap::new();
        routes.insert(
            "app.mikrom.local".to_string(),
            Route {
                host: "app.mikrom.local".to_string(),
                targets: vec!["10.0.0.1:8080".to_string(), "10.0.0.2:8080".to_string()],
            },
        );

        let state = Arc::new(RwLock::new(State {
            routes,
            acme_tokens: HashMap::new(),
            certificates: HashMap::new(),
        }));

        let metrics = Arc::new(RouterMetricsCounters::new());
        let proxy = MikromProxy::new(state, false, metrics);

        let t1 = proxy.select_target("app.mikrom.local").await.unwrap();
        let t2 = proxy.select_target("app.mikrom.local").await.unwrap();
        let t3 = proxy.select_target("app.mikrom.local").await.unwrap();

        assert_eq!(t1, "10.0.0.1:8080");
        assert_eq!(t2, "10.0.0.2:8080");
        assert_eq!(t3, "10.0.0.1:8080");
    }
}
