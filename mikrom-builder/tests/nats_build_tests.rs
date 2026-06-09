use futures::StreamExt;
use mikrom_proto::builder::{BuildProgress, BuildRequest, BuildResponse, GetBuildStatusResponse};
use prost::Message;
use std::time::Duration;

fn nats_integration_enabled() -> bool {
    if std::env::var("MIKROM_RUN_NATS_TESTS").is_err() {
        println!("Skipping NATS test: set MIKROM_RUN_NATS_TESTS=1 to run it");
        return false;
    }

    true
}

#[tokio::test]
#[ignore = "requires a NATS broker; run with MIKROM_RUN_NATS_TESTS=1 cargo test -p mikrom-builder --test nats_build_tests -- --ignored"]
async fn test_builder_nats_flow() {
    if !nats_integration_enabled() {
        return;
    }

    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let client = async_nats::connect(&nats_url)
        .await
        .expect("Failed to connect to NATS");

    let build_id = format!("build-{}", uuid::Uuid::new_v4());
    let subject = format!("test.builder.build.{}", uuid::Uuid::new_v4());
    let status_subject = format!("mikrom.builder.{}.status", build_id);

    // 1. Subscribe to status updates
    let mut status_sub = client.subscribe(status_subject.clone()).await.unwrap();
    client.flush().await.unwrap();

    // 2. Simulate Builder processing
    let build_id_clone = build_id.clone();
    let client_clone = client.clone();
    let queue_group = format!("test-builders-{}", uuid::Uuid::new_v4());
    let subject_clone = subject.clone();
    tokio::spawn(async move {
        let mut build_sub = client_clone
            .queue_subscribe(subject_clone, queue_group)
            .await
            .unwrap();
        client_clone.flush().await.unwrap();
        if let Some(msg) = build_sub.next().await {
            let req = BuildRequest::decode(&msg.payload[..]).unwrap();

            // Send initial response
            if let Some(reply) = msg.reply {
                let resp = BuildResponse {
                    success: true,
                    build_id: build_id_clone.clone(),
                    message: "Build started".to_string(),
                };
                let mut buf = Vec::new();
                resp.encode(&mut buf).unwrap();
                client_clone.publish(reply, buf.into()).await.unwrap();
            }

            // Simulate build progress
            tokio::time::sleep(Duration::from_millis(100)).await;
            let status = GetBuildStatusResponse {
                build_id: build_id_clone.clone(),
                status: 3, // Success
                image_tag: format!("registry.mikrom.io/{}:latest", req.image_name),
                message: "Build complete".to_string(),
                exposed_port: 8080,
                ..Default::default()
            };
            let mut buf = Vec::new();
            status.encode(&mut buf).unwrap();
            client_clone
                .publish(
                    format!("mikrom.builder.{}.status", build_id_clone),
                    buf.into(),
                )
                .await
                .unwrap();
        }
    });

    // 3. API triggers build
    tokio::time::sleep(Duration::from_millis(200)).await;
    let req = BuildRequest {
        app_id: "app-1".to_string(),
        git_url: "https://github.com/test/repo".to_string(),
        image_name: "test-app".to_string(),
        tag: "latest".to_string(),
        git_auth_token: None,
    };
    let mut buf = Vec::new();
    req.encode(&mut buf).unwrap();

    let resp_msg =
        tokio::time::timeout(Duration::from_secs(10), client.request(subject, buf.into()))
            .await
            .expect("Timeout waiting for builder response")
            .expect("Request failed");
    let resp = BuildResponse::decode(&resp_msg.payload[..]).unwrap();
    assert!(resp.success);
    assert_eq!(resp.build_id, build_id);

    // 4. Wait for status update
    let status_msg = tokio::time::timeout(Duration::from_secs(10), status_sub.next())
        .await
        .expect("Timeout waiting for status update")
        .expect("No status message");

    let status = GetBuildStatusResponse::decode(&status_msg.payload[..]).unwrap();
    assert_eq!(status.status, 3); // Success
    assert_eq!(status.exposed_port, 8080);
}

#[tokio::test]
#[ignore = "requires a NATS broker; run with MIKROM_RUN_NATS_TESTS=1 cargo test -p mikrom-builder --test nats_build_tests -- --ignored"]
async fn test_builder_progress_streaming() {
    if !nats_integration_enabled() {
        return;
    }

    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let client = async_nats::connect(&nats_url)
        .await
        .expect("Failed to connect to NATS");

    let build_id = format!("build-{}", uuid::Uuid::new_v4());
    let progress_subject = format!("mikrom.builder.{}.progress.test", build_id);

    let mut progress_sub = client.subscribe(progress_subject.clone()).await.unwrap();
    client.flush().await.unwrap();

    // Simulate Builder publishing progress
    let client_clone = client.clone();
    let build_id_clone = build_id.clone();
    let subject_clone = progress_subject.clone();
    tokio::spawn(async move {
        for i in 1..=3 {
            let progress = BuildProgress {
                build_id: build_id_clone.clone(),
                message: format!("Step {}", i),
                percent: i as f32 * 33.3,
            };
            let mut buf = Vec::new();
            progress.encode(&mut buf).unwrap();
            client_clone
                .publish(subject_clone.clone(), buf.into())
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });

    // API side receives progress
    for i in 1..=3 {
        let msg = tokio::time::timeout(Duration::from_secs(5), progress_sub.next())
            .await
            .expect("Timeout waiting for progress")
            .expect("No progress message");

        let progress = BuildProgress::decode(&msg.payload[..]).unwrap();
        assert_eq!(progress.message, format!("Step {}", i));
        assert!(progress.percent > 0.0);
    }
}
