#[cfg(test)]
mod tests {
    use crate::traffic::RouterTrafficPublisher;
    use std::time::Duration;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_traffic_publisher_deduplication() {
        let (tx, mut rx) = mpsc::channel(100);
        let publisher = RouterTrafficPublisher::new("router-1".to_string(), tx);

        // Send same hostname multiple times
        publisher.record("app.local".to_string());
        publisher.record("app.local".to_string());
        publisher.record("app.local".to_string());

        // Should only receive one event
        let first = rx.recv().await.expect("Should receive first event");
        assert_eq!(first.hostname, "app.local");

        // Use a small timeout to confirm no second message arrived
        let second = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await;
        assert!(
            second.is_err(),
            "Should have deduplicated subsequent events"
        );
    }

    #[tokio::test]
    async fn test_traffic_publisher_different_hosts() {
        let (tx, mut rx) = mpsc::channel(100);
        let publisher = RouterTrafficPublisher::new("router-1".to_string(), tx);

        publisher.record("app1.local".to_string());
        publisher.record("app2.local".to_string());

        let ev1 = rx.recv().await.expect("Should receive app1 event");
        let ev2 = rx.recv().await.expect("Should receive app2 event");

        assert_eq!(ev1.hostname, "app1.local");
        assert_eq!(ev2.hostname, "app2.local");
    }
}
