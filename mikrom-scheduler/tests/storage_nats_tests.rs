use futures::StreamExt;
use mikrom_proto::agent::{AgentCommand, AgentCommandResponse};
use mikrom_proto::scheduler::{CloneVolumeRequest, RestoreSnapshotRequest};
use mikrom_scheduler::application::AppService;
use mikrom_scheduler::domain::worker::{MockJobRepository, MockWorkerRepository, Worker};
use mikrom_scheduler::infrastructure::nats::NatsEventLoop;
use mikrom_scheduler::server::SchedulerServer;
use prost::Message;
use std::sync::Arc;
use tokio::time::{Duration, timeout};

async fn connect_nats_or_skip() -> Option<async_nats::Client> {
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    match async_nats::connect(&nats_url).await {
        Ok(client) => Some(client),
        Err(err) => {
            eprintln!("Skipping scheduler storage test: failed to connect to NATS: {err}");
            None
        },
    }
}

#[tokio::test]
async fn test_scheduler_storage_nats_dispatch() {
    let Some(client) = connect_nats_or_skip().await else {
        return;
    };

    // 1. Mock dependencies
    let mut job_repo = MockJobRepository::new();
    job_repo.expect_list_jobs().returning(|_, _, _| Ok(vec![]));

    // Correct way: AppService needs the REAL mocked repos
    let mut app_service = AppService::new(
        Arc::new(job_repo),
        Arc::new(mikrom_scheduler::domain::app::MockAppRepository::new()),
        Arc::new(MockWorkerRepository::new()), // Will re-mock below
        Arc::new(mikrom_scheduler::infrastructure::nats::NatsAgentClient::new(client.clone())),
        client.clone(),
        sqlx::PgPool::connect_lazy("postgres://localhost/dummy").unwrap(),
        900,
    );

    let mut worker_repo = MockWorkerRepository::new();
    let test_worker = Worker {
        host_id: "test-host".to_string(),
        hostname: "test".to_string(),
        advertise_address: "127.0.0.1".to_string(),
        wireguard_pubkey: None,
        wireguard_ip: None,
        wireguard_port: None,
        metrics: None,
        registered_at: 0,
        last_heartbeat: chrono::Utc::now().timestamp(),
    };
    let test_worker_clone = test_worker.clone();
    worker_repo
        .expect_get_available_workers()
        .returning(move |_| Ok(vec![test_worker_clone.clone()]));
    worker_repo
        .expect_list_workers()
        .returning(move || Ok(vec![test_worker.clone()]));
    app_service.worker_repo = Arc::new(worker_repo);

    let server = SchedulerServer {
        app_service: Arc::new(app_service),
        certs: None,
    };

    let event_loop = NatsEventLoop::new(server, client.clone());

    // Start event loop in background
    let loop_handle = tokio::spawn(async move {
        let _ = event_loop.run().await;
    });

    // Wait for subscriptions to settle
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 3. Test Clone Dispatch
    let mut agent_sub = client
        .subscribe("mikrom.agent.test-host.cmd")
        .await
        .unwrap();

    let clone_req = CloneVolumeRequest {
        source_volume_id: "vol-1".to_string(),
        snapshot_name: "snap-1".to_string(),
        target_volume_id: "vol-2".to_string(),
        pool_name: "pool-1".to_string(),
        host_id: "".to_string(), // Should pick test-host
    };

    let mut payload = Vec::new();
    clone_req.encode(&mut payload).unwrap();

    let reply = client.new_inbox();
    let _reply_sub = client.subscribe(reply.clone()).await.unwrap();

    client
        .publish_with_reply("mikrom.scheduler.clone_volume", reply, payload.into())
        .await
        .unwrap();

    // Expect Agent Command
    let agent_msg = timeout(Duration::from_secs(5), agent_sub.next())
        .await
        .expect("Timeout waiting for agent command")
        .expect("No agent command received");

    let agent_cmd = AgentCommand::decode(&agent_msg.payload[..]).unwrap();
    match agent_cmd.command.unwrap() {
        mikrom_proto::agent::agent_command::Command::CloneVolume(req) => {
            assert_eq!(req.source_volume_id, "vol-1");
            assert_eq!(req.target_volume_id, "vol-2");
        },
        _ => panic!("Expected CloneVolume command"),
    }

    // Respond from Agent
    let agent_resp = AgentCommandResponse {
        success: true,
        message: "Cloned!".to_string(),
    };
    let mut resp_payload = Vec::new();
    agent_resp.encode(&mut resp_payload).unwrap();
    client
        .publish(agent_msg.reply.unwrap(), resp_payload.into())
        .await
        .unwrap();

    // 4. Test Restore Dispatch
    let restore_req = RestoreSnapshotRequest {
        volume_id: "vol-1".to_string(),
        snapshot_name: "snap-1".to_string(),
        pool_name: "pool-1".to_string(),
        host_id: "".to_string(),
    };

    let mut payload = Vec::new();
    restore_req.encode(&mut payload).unwrap();

    let reply = client.new_inbox();

    client
        .publish_with_reply("mikrom.scheduler.restore_snapshot", reply, payload.into())
        .await
        .unwrap();

    let agent_msg = timeout(Duration::from_secs(5), agent_sub.next())
        .await
        .expect("Timeout waiting for agent command")
        .expect("No agent command received");

    let agent_cmd = AgentCommand::decode(&agent_msg.payload[..]).unwrap();
    match agent_cmd.command.unwrap() {
        mikrom_proto::agent::agent_command::Command::RestoreSnapshot(req) => {
            assert_eq!(req.volume_id, "vol-1");
            assert_eq!(req.snapshot_name, "snap-1");
        },
        _ => panic!("Expected RestoreSnapshot command"),
    }

    loop_handle.abort();
}
